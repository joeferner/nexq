//! Where each SQS operation is handled.
//!
//! Routing ends here: the request has been recognised as a specific [`Operation`] and
//! its input decoded. Each operation gets its own handler as the engine behind it
//! lands; until then they are all reported as not implemented, which is a different
//! answer to the client than "no such operation".

use serde_json::{Map, Value};

use crate::error::ApiError;
use crate::protocol::Operation;

/// Invoke an operation.
pub async fn dispatch(operation: Operation, _input: Map<String, Value>) -> Result<Value, ApiError> {
    Err(ApiError::not_implemented(operation))
}
