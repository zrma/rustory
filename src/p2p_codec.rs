use async_trait::async_trait;
use futures::prelude::*;
use libp2p::StreamProtocol;
use serde::{Serialize, de::DeserializeOwned};
use std::{io, marker::PhantomData};

#[derive(Clone)]
pub struct JsonCodec<Req, Resp> {
    request_size_maximum: u64,
    response_size_maximum: u64,
    phantom: PhantomData<(Req, Resp)>,
}

impl<Req, Resp> JsonCodec<Req, Resp> {
    pub fn new(request_size_maximum: u64, response_size_maximum: u64) -> Self {
        Self {
            request_size_maximum,
            response_size_maximum,
            phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<Req, Resp> libp2p_request_response::Codec for JsonCodec<Req, Resp>
where
    Req: Send + Serialize + DeserializeOwned,
    Resp: Send + Serialize + DeserializeOwned,
{
    type Protocol = StreamProtocol;
    type Request = Req;
    type Response = Resp;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Req>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_limited_bytes(io, self.request_size_maximum).await?;
        serde_json::from_slice(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Resp>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_limited_bytes(io, self.response_size_maximum).await?;
        serde_json::from_slice(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&req)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        ensure_len_le(data.len(), self.request_size_maximum, "request")?;
        io.write_all(data.as_ref()).await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data = serde_json::to_vec(&resp)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        ensure_len_le(data.len(), self.response_size_maximum, "response")?;
        io.write_all(data.as_ref()).await?;
        Ok(())
    }
}

async fn read_limited_bytes<T>(io: &mut T, max: u64) -> io::Result<Vec<u8>>
where
    T: AsyncRead + Unpin + Send,
{
    let mut vec = Vec::new();

    // `max+1`까지 읽어서 실제 초과 여부를 판별한다(기존 JSON codec의 “잘린 JSON 파싱 실패” 방지).
    io.take(max.saturating_add(1)).read_to_end(&mut vec).await?;

    if vec.len() as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message too large: {} > {} bytes", vec.len(), max),
        ));
    }

    Ok(vec)
}

fn ensure_len_le(len: usize, max: u64, kind: &str) -> io::Result<()> {
    if len as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{kind} too large: {len} > {max} bytes"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor;
    use libp2p_request_response::Codec;
    use serde::Deserialize;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestReq {
        payload: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    struct TestResp {
        payload: String,
    }

    #[test]
    fn write_request_fails_when_too_large() {
        let mut codec = JsonCodec::<TestReq, TestResp>::new(10, 10);
        let protocol = StreamProtocol::new("/test/1");
        let req = TestReq {
            payload: "x".repeat(100),
        };

        let mut io = futures::io::Cursor::new(Vec::new());
        let err = executor::block_on(codec.write_request(&protocol, &mut io, req)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("request too large"));
    }

    #[test]
    fn read_request_fails_with_clear_error_when_too_large() {
        let mut codec = JsonCodec::<TestReq, TestResp>::new(10, 10);
        let protocol = StreamProtocol::new("/test/1");

        // 11 bytes > max(10)
        let mut io = futures::io::Cursor::new(vec![b'a'; 11]);
        let err = executor::block_on(codec.read_request(&protocol, &mut io)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("message too large"));
    }
}
