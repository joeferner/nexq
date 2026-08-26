//! Message checksums.
//!
//! SQS reports an MD5 of the body on both send and receive, and one of the message
//! attributes when there are any. SDKs *verify* both — a missing or wrong value makes a
//! client raise rather than accept the message. So this is not decoration: it is part of
//! being compatible.
//!
//! MD5 is not being used as a security primitive here. It detects content mangled in
//! transit, and the algorithm is fixed by the wire format rather than chosen.

use md5::{Digest, Md5};
use nexq_core::model::{AttributeValue, MessageAttributes};

/// Marks a value carried as text in the attribute digest. Covers `Number` as well as
/// `String`, since both travel in `StringValue`.
const TEXT_TRANSPORT: u8 = 1;

/// Marks a value carried as bytes in the attribute digest.
const BINARY_TRANSPORT: u8 = 2;

/// Hex-encoded MD5 of a message body, as SQS reports it.
pub fn md5_of_body(body: &str) -> String {
    hex::encode(Md5::digest(body.as_bytes()))
}

/// Hex-encoded MD5 of a message's attributes, as SQS computes it.
///
/// The encoding is SQS's and is not negotiable, since an SDK computes the same digest
/// independently and compares. For each attribute, in name order:
///
/// 1. the name, as a big-endian `u32` byte length followed by its UTF-8 bytes
/// 2. the data type, framed the same way
/// 3. one byte saying how the value travels — `1` for text, `2` for bytes — then the
///    value, framed the same way
///
/// Name order comes free from [`MessageAttributes`] being a `BTreeMap`, whose iteration
/// order is exactly the UTF-8 byte order the digest is defined over.
///
/// Verified against digests published by AWS itself — see the tests.
pub fn md5_of_attributes(attributes: &MessageAttributes) -> String {
    let mut digest = Md5::new();

    for (name, attribute) in attributes {
        digest.update(framed(name.as_bytes()));
        digest.update(framed(attribute.data_type.as_bytes()));

        match &attribute.value {
            AttributeValue::Text(text) => {
                digest.update([TEXT_TRANSPORT]);
                digest.update(framed(text.as_bytes()));
            }
            AttributeValue::Binary(bytes) => {
                digest.update([BINARY_TRANSPORT]);
                digest.update(framed(bytes));
            }
        }
    }

    hex::encode(digest.finalize())
}

/// A length-prefixed field: a big-endian `u32` length, then the bytes.
///
/// The length is what separates one field from the next, so without it two different
/// attribute sets could hash the same — `{"ab": "c"}` and `{"a": "bc"}` would be
/// indistinguishable.
fn framed(bytes: &[u8]) -> Vec<u8> {
    // A value long enough to overflow a u32 cannot reach here: the message size limit
    // is orders of magnitude below it, and is checked before a message is stored.
    let length = u32::try_from(bytes.len()).unwrap_or(u32::MAX);

    let mut framed = Vec::with_capacity(4 + bytes.len());
    framed.extend_from_slice(&length.to_be_bytes());
    framed.extend_from_slice(bytes);

    framed
}

#[cfg(test)]
mod tests {
    use nexq_core::model::MessageAttribute;

    use super::*;

    #[test]
    fn matches_known_md5_values() {
        // Published vectors, so this is checked against MD5 itself rather than against
        // whatever this code happens to produce.
        assert_eq!(md5_of_body(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_of_body("hello"), "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(
            md5_of_body("The quick brown fox jumps over the lazy dog"),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
    }

    #[test]
    fn is_hex_of_the_expected_width() {
        let digest = md5_of_body("anything");

        assert_eq!(digest.len(), 32);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(
            digest.chars().all(|c| !c.is_ascii_uppercase()),
            "SQS reports lowercase hex"
        );
    }

    #[test]
    fn covers_multibyte_bodies_by_their_bytes() {
        // Hashing chars rather than bytes would give a different answer for the same
        // wire content.
        assert_eq!(md5_of_body("é"), "66ddcd97cfdeabb2f6fb8a999b4bc76f");
    }

    /// Attributes from a list of `(name, data type, value)`, all text.
    fn text_attributes(entries: &[(&str, &str, &str)]) -> MessageAttributes {
        entries
            .iter()
            .map(|(name, data_type, value)| {
                (
                    (*name).to_owned(),
                    MessageAttribute {
                        data_type: (*data_type).to_owned(),
                        value: AttributeValue::Text((*value).to_owned()),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn reproduces_attribute_digests_that_aws_published() {
        // These come from the `aws-cli` documentation examples, whose outputs are real
        // SQS responses. AWS elides the middle of each digest, so the assertion checks
        // the ends — 64 bits of a digest AWS computed, which a wrong encoding would not
        // hit. Nothing here rests on reading the spec correctly.
        let cases: [(&str, MessageAttributes, &str, &str); 4] = [
            (
                "receive-message, all attributes",
                text_attributes(&[
                    ("PostalCode", "String", "ABC123"),
                    ("City", "String", "Any City"),
                ]),
                "9424c491",
                "26bc3ae7",
            ),
            (
                // The same message, but only one attribute was asked for — and AWS's
                // digest differs from the case above. This is what proves the digest on
                // receive covers what is *returned*, not everything the message holds.
                "receive-message, one attribute of two",
                text_attributes(&[("PostalCode", "String", "ABC123")]),
                "b8e89563",
                "e088e74f",
            ),
            (
                "send-message-batch, first entry",
                text_attributes(&[
                    ("SellerName", "String", "Example Store"),
                    ("City", "String", "Any City"),
                    ("Region", "String", "WA"),
                    ("PostalCode", "String", "99065"),
                    ("PricePerGallon", "Number", "1.99"),
                ]),
                "10809b55",
                "baf283ef",
            ),
            (
                "send-message-batch, second entry",
                text_attributes(&[
                    ("SellerName", "String", "Example Fuels"),
                    ("City", "String", "North Town"),
                    ("Region", "String", "WA"),
                    ("PostalCode", "String", "99123"),
                    ("PricePerGallon", "Number", "1.87"),
                ]),
                "55623928",
                "ae354a25",
            ),
        ];

        for (label, attributes, starts, ends) in cases {
            let digest = md5_of_attributes(&attributes);

            assert!(
                digest.starts_with(starts) && digest.ends_with(ends),
                "{label}: {digest} does not match AWS's {starts}...{ends}"
            );
        }
    }

    #[test]
    fn reproduces_an_aws_digest_covering_a_binary_value() {
        // Also from the `aws-cli` documentation, the one example carrying a `Binary`
        // attribute, which is what pins the binary transport marker. Its value is the
        // ASCII of `SGVsbG8sIFdvcmxkIQ==` rather than the `Hello, World!` those
        // characters decode to: the CLI of the day base64-encoded a literal string from
        // a JSON input file and then encoded the blob again on the wire, so what SQS
        // stored — and hashed — was the base64 text itself.
        let attributes: MessageAttributes = [
            (
                "City".to_owned(),
                MessageAttribute {
                    data_type: "String".to_owned(),
                    value: AttributeValue::Text("Any City".to_owned()),
                },
            ),
            (
                "Greeting".to_owned(),
                MessageAttribute {
                    data_type: "Binary".to_owned(),
                    value: AttributeValue::Binary(b"SGVsbG8sIFdvcmxkIQ==".to_vec()),
                },
            ),
            (
                "Population".to_owned(),
                MessageAttribute {
                    data_type: "Number".to_owned(),
                    value: AttributeValue::Text("1250800".to_owned()),
                },
            ),
        ]
        .into();

        assert_eq!(
            md5_of_attributes(&attributes),
            "00484c6852e072874de421e059e48f06",
            "AWS published 00484c68...59e48f06 for exactly these attributes"
        );
    }

    #[test]
    fn no_attributes_hash_to_the_digest_of_nothing() {
        assert_eq!(
            md5_of_attributes(&MessageAttributes::new()),
            md5_of_body(""),
            "an empty digest, which is why the field is omitted rather than sent"
        );
    }

    #[test]
    fn the_digest_does_not_depend_on_insertion_order() {
        // Two clients sending the same attributes in different orders must produce the
        // same digest, or one of them would reject its own message.
        let one = text_attributes(&[("b", "String", "2"), ("a", "String", "1")]);
        let other = text_attributes(&[("a", "String", "1"), ("b", "String", "2")]);

        assert_eq!(md5_of_attributes(&one), md5_of_attributes(&other));
    }

    #[test]
    fn framing_keeps_apart_what_would_otherwise_run_together() {
        // Without a length prefix these would hash identically, and a client could be
        // handed an attribute set that checksums as a different one.
        let split_one_way = text_attributes(&[("ab", "String", "c")]);
        let split_another = text_attributes(&[("a", "String", "bc")]);

        assert_ne!(
            md5_of_attributes(&split_one_way),
            md5_of_attributes(&split_another)
        );
    }

    #[test]
    fn text_and_binary_values_are_distinguished() {
        // The transport marker is the only difference, and it has to matter: the same
        // bytes as text and as binary are different attributes.
        let as_text = text_attributes(&[("x", "String", "hello")]);
        let as_binary: MessageAttributes = [(
            "x".to_owned(),
            MessageAttribute {
                data_type: "String".to_owned(),
                value: AttributeValue::Binary(b"hello".to_vec()),
            },
        )]
        .into();

        assert_ne!(md5_of_attributes(&as_text), md5_of_attributes(&as_binary));
    }

    #[test]
    fn the_data_type_is_part_of_the_digest() {
        // A custom label is a real difference a client can see, so it must change the
        // digest rather than being cosmetic.
        let plain = text_attributes(&[("x", "String", "1")]);
        let labelled = text_attributes(&[("x", "String.uuid", "1")]);

        assert_ne!(md5_of_attributes(&plain), md5_of_attributes(&labelled));
    }
}
