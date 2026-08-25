//! SQS- and SNS-compatible facades: compatibility-only translation layers over the
//! core operation set. One crate because both need SigV4 verification and the same AWS
//! wire encoding and error shapes.
