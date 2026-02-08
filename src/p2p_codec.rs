use async_trait::async_trait;
use futures::prelude::*;
use libp2p::StreamProtocol;
use serde::{Serialize, de::DeserializeOwned};
use std::{io, marker::PhantomData};

#[derive(Clone)]
pub struct JsonCodec<Req, Resp> {
    request_wire_maximum: u64,
    response_wire_maximum: u64,

    // 압축 프로토콜인 경우, decode(압축 해제 후 JSON bytes) 최대 크기를 별도로 제한한다.
    request_decoded_maximum: u64,
    response_decoded_maximum: u64,
    phantom: PhantomData<(Req, Resp)>,
}

impl<Req, Resp> JsonCodec<Req, Resp> {
    pub fn new(request_wire_maximum: u64, response_wire_maximum: u64) -> Self {
        Self {
            request_wire_maximum,
            response_wire_maximum,
            request_decoded_maximum: request_wire_maximum,
            response_decoded_maximum: response_wire_maximum,
            phantom: PhantomData,
        }
    }

    pub fn with_decoded_maximum(
        mut self,
        request_decoded_maximum: u64,
        response_decoded_maximum: u64,
    ) -> Self {
        self.request_decoded_maximum = request_decoded_maximum;
        self.response_decoded_maximum = response_decoded_maximum;
        self
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

    async fn read_request<T>(&mut self, protocol: &Self::Protocol, io: &mut T) -> io::Result<Req>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_limited_bytes(io, self.request_wire_maximum).await?;
        let data = if is_zstd_protocol(protocol) {
            decompress_zstd_limited(&data, self.request_decoded_maximum)?
        } else {
            data
        };
        serde_json::from_slice(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    async fn read_response<T>(&mut self, protocol: &Self::Protocol, io: &mut T) -> io::Result<Resp>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_limited_bytes(io, self.response_wire_maximum).await?;
        let data = if is_zstd_protocol(protocol) {
            decompress_zstd_limited(&data, self.response_decoded_maximum)?
        } else {
            data
        };
        serde_json::from_slice(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    async fn write_request<T>(
        &mut self,
        protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let mut data = serde_json::to_vec(&req)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

        if is_zstd_protocol(protocol) {
            ensure_len_le(data.len(), self.request_decoded_maximum, "decoded request")?;
            data = compress_zstd(&data)?;
        }

        ensure_len_le(data.len(), self.request_wire_maximum, "request")?;
        io.write_all(data.as_ref()).await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        protocol: &Self::Protocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let mut data = serde_json::to_vec(&resp)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

        if is_zstd_protocol(protocol) {
            ensure_len_le(
                data.len(),
                self.response_decoded_maximum,
                "decoded response",
            )?;
            data = compress_zstd(&data)?;
        }

        ensure_len_le(data.len(), self.response_wire_maximum, "response")?;
        io.write_all(data.as_ref()).await?;
        Ok(())
    }
}

fn is_zstd_protocol(protocol: &StreamProtocol) -> bool {
    protocol.as_ref().ends_with("/1.0.1")
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

fn compress_zstd(data: &[u8]) -> io::Result<Vec<u8>> {
    zstd::stream::encode_all(std::io::Cursor::new(data), 1)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn decompress_zstd_limited(data: &[u8], max: u64) -> io::Result<Vec<u8>> {
    use std::io::Read;

    let decoder = zstd::stream::read::Decoder::new(std::io::Cursor::new(data))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut limited = decoder.take(max.saturating_add(1));
    let mut out = Vec::new();
    limited
        .read_to_end(&mut out)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

    if out.len() as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "decompressed message too large: {} > {} bytes",
                out.len(),
                max
            ),
        ));
    }

    Ok(out)
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

    #[test]
    fn zstd_protocol_roundtrip_compresses_payload() {
        let mut codec = JsonCodec::<TestReq, TestResp>::new(10_000, 10_000)
            .with_decoded_maximum(100_000, 100_000);
        let protocol = StreamProtocol::new("/test/1.0.1");
        let req = TestReq {
            payload: "hello world ".repeat(100),
        };

        let mut io = futures::io::Cursor::new(Vec::new());
        executor::block_on(codec.write_request(&protocol, &mut io, req.clone())).unwrap();
        let buf = io.into_inner();

        // zstd frame magic: 28 B5 2F FD
        assert!(buf.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]));

        let mut io = futures::io::Cursor::new(buf);
        let got = executor::block_on(codec.read_request(&protocol, &mut io)).unwrap();
        assert_eq!(got, req);
    }

    #[test]
    fn read_request_fails_when_decompressed_too_large() {
        let mut codec =
            JsonCodec::<TestReq, TestResp>::new(10_000, 10_000).with_decoded_maximum(10, 10);
        let protocol = StreamProtocol::new("/test/1.0.1");
        let req = TestReq {
            payload: "x".repeat(100),
        };

        let json = serde_json::to_vec(&req).unwrap();
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(json), 1).unwrap();
        let mut io = futures::io::Cursor::new(compressed);
        let err = executor::block_on(codec.read_request(&protocol, &mut io)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert!(err.to_string().contains("too large"));
    }
}
