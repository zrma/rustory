use rusqlite::types::ValueRef;

pub(super) fn row_lossy_string(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<String> {
    Ok(value_ref_to_lossy_string(row.get_ref(index)?))
}

pub(super) fn row_lossy_opt_string(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<Option<String>> {
    let value = row.get_ref(index)?;
    Ok(match value {
        ValueRef::Null => None,
        _ => Some(value_ref_to_lossy_string(value)),
    })
}

fn value_ref_to_lossy_string(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => value.to_string(),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_ref_conversion_preserves_scalars_and_decodes_bytes_lossily() {
        assert_eq!(value_ref_to_lossy_string(ValueRef::Null), "");
        assert_eq!(value_ref_to_lossy_string(ValueRef::Integer(42)), "42");
        assert_eq!(value_ref_to_lossy_string(ValueRef::Real(1.5)), "1.5");
        assert_eq!(
            value_ref_to_lossy_string(ValueRef::Blob(b"invalid-\xff")),
            "invalid-\u{fffd}"
        );
    }
}
