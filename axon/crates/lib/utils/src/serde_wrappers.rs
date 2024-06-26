use std::str::FromStr;

use bigdecimal::BigDecimal;
use num::{rational::Ratio, BigUint};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::convert::*;

#[derive(Clone, Debug)]
pub struct UnsignedRatioSerializeAsDecimal;
impl UnsignedRatioSerializeAsDecimal {
    pub fn serialize<S>(value: &Ratio<BigUint>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            BigDecimal::serialize(&ratio_to_big_decimal(value, 18), serializer)
        } else {
            value.serialize(serializer)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Ratio<BigUint>, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            // First, deserialize a string value. It is expected to be a
            // hexadecimal representation of `BigDecimal`.
            let big_decimal_string = BigDecimal::deserialize(deserializer)?;

            big_decimal_to_ratio(&big_decimal_string).map_err(de::Error::custom)
        } else {
            Ratio::<BigUint>::deserialize(deserializer)
        }
    }

    pub fn deserialize_from_str_with_dot(input: &str) -> Result<Ratio<BigUint>, anyhow::Error> {
        big_decimal_to_ratio(&BigDecimal::from_str(input)?)
    }

    pub fn serialize_to_str_with_dot(num: &Ratio<BigUint>, precision: usize) -> String {
        ratio_to_big_decimal(num, precision)
            .to_string()
            .trim_end_matches('0')
            .to_string()
    }
}

/// Trait for specifying prefix for bytes to hex serialization
pub trait Prefix {
    fn prefix() -> &'static str;
}

/// "0x" hex prefix
pub struct ZeroxPrefix;
impl Prefix for ZeroxPrefix {
    fn prefix() -> &'static str {
        "0x"
    }
}

/// Used to annotate `Vec<u8>` fields that you want to serialize like hex-encoded string with prefix
/// Use this struct in annotation like that `[serde(with = "BytesToHexSerde::<T>"]`
/// where T is concrete prefix type (e.g. `AxonBlockPrefix`)
pub struct BytesToHexSerde<P> {
    _marker: std::marker::PhantomData<P>,
}

impl<P: Prefix> BytesToHexSerde<P> {
    pub fn serialize<S>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // First, serialize to hexadecimal string.
        let hex_value = format!(
            "{}{}",
            P::prefix(),
            axon_primitives::web3::hex::encode(value)
        );

        // Then, serialize it using `Serialize` trait implementation for `String`.
        String::serialize(&hex_value, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let deserialized_string = String::deserialize(deserializer)?;

        if let Some(deserialized_string) = deserialized_string.strip_prefix(P::prefix()) {
            axon_primitives::web3::hex::decode(deserialized_string).map_err(de::Error::custom)
        } else {
            Err(de::Error::custom(format!(
                "string value missing prefix: {:?}",
                P::prefix()
            )))
        }
    }
}

pub type ZeroPrefixHexSerde = BytesToHexSerde<ZeroxPrefix>;

#[cfg(test)]
mod test {
    use super::*;

    /// Tests that `Ratio` serializer works correctly.
    #[test]
    fn test_ratio_serialize_as_decimal() {
        #[derive(Clone, Serialize, Deserialize)]
        struct RatioSerdeWrapper(
            #[serde(with = "UnsignedRatioSerializeAsDecimal")] pub Ratio<BigUint>,
        );
        // It's essential that this number is a finite decimal, otherwise the precision will be lost
        // and the assertion will fail.
        let expected = RatioSerdeWrapper(Ratio::new(
            BigUint::from(120315391195132u64),
            BigUint::from(1250000000u64),
        ));
        let value =
            serde_json::to_value(expected.clone()).expect("cannot serialize Ratio as Decimal");
        let ratio: RatioSerdeWrapper =
            serde_json::from_value(value).expect("cannot deserialize Ratio from Decimal");
        assert_eq!(expected.0, ratio.0);
    }
}
