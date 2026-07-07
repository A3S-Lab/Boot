use crate::{BootError, Result};
use percent_encoding::percent_decode_str;

pub(crate) fn decode_percent_encoded(value: &str) -> Result<String> {
    validate_percent_encoding(value)?;
    percent_decode_str(value)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|err| BootError::BadRequest(err.to_string()))
}

pub(crate) fn validate_percent_encoding(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(BootError::BadRequest(format!(
                    "invalid percent encoding: {value}"
                )));
            }
            index += 3;
        } else {
            index += 1;
        }
    }

    Ok(())
}
