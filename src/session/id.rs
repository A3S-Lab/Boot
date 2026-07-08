use crate::{BootError, Result};

pub(super) fn generate_session_id() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| BootError::Internal(format!("failed to generate session id: {error}")))?;
    Ok(bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

pub(super) fn validate_session_id(session_id: String) -> Result<String> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty()
        || session_id.contains(char::is_whitespace)
        || session_id.contains([';', ',', '='])
    {
        return Err(BootError::BadRequest(format!(
            "invalid session id: {session_id:?}"
        )));
    }
    Ok(session_id)
}
