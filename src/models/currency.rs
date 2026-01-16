use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// ISO 4217 Currency codes supported by the settlement engine.
/// This enum represents the most common currencies used in financial transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum Currency {
    USD,
    EUR,
    GBP,
    JPY,
    CHF,
    CAD,
    AUD,
    NZD,
    CNY,
    HKD,
    SGD,
    INR,
    BRL,
    MXN,
    ZAR,
    AED,
    SAR,
    KRW,
    THB,
    MYR,
}

impl Currency {
    /// Returns the ISO 4217 numeric code for the currency.
    pub fn numeric_code(&self) -> u16 {
        match self {
            Currency::USD => 840,
            Currency::EUR => 978,
            Currency::GBP => 826,
            Currency::JPY => 392,
            Currency::CHF => 756,
            Currency::CAD => 124,
            Currency::AUD => 36,
            Currency::NZD => 554,
            Currency::CNY => 156,
            Currency::HKD => 344,
            Currency::SGD => 702,
            Currency::INR => 356,
            Currency::BRL => 986,
            Currency::MXN => 484,
            Currency::ZAR => 710,
            Currency::AED => 784,
            Currency::SAR => 682,
            Currency::KRW => 410,
            Currency::THB => 764,
            Currency::MYR => 458,
        }
    }

    /// Returns the number of decimal places for the currency.
    pub fn decimal_places(&self) -> u8 {
        match self {
            Currency::JPY | Currency::KRW => 0,
            _ => 2,
        }
    }

    /// Returns the currency symbol.
    pub fn symbol(&self) -> &'static str {
        match self {
            Currency::USD => "$",
            Currency::EUR => "E",
            Currency::GBP => "P",
            Currency::JPY => "Y",
            Currency::CHF => "CHF",
            Currency::CAD => "C$",
            Currency::AUD => "A$",
            Currency::NZD => "NZ$",
            Currency::CNY => "CN",
            Currency::HKD => "HK$",
            Currency::SGD => "S$",
            Currency::INR => "Rs",
            Currency::BRL => "R$",
            Currency::MXN => "MX$",
            Currency::ZAR => "R",
            Currency::AED => "AED",
            Currency::SAR => "SAR",
            Currency::KRW => "W",
            Currency::THB => "B",
            Currency::MYR => "RM",
        }
    }

    /// Returns the currency name.
    pub fn name(&self) -> &'static str {
        match self {
            Currency::USD => "US Dollar",
            Currency::EUR => "Euro",
            Currency::GBP => "British Pound",
            Currency::JPY => "Japanese Yen",
            Currency::CHF => "Swiss Franc",
            Currency::CAD => "Canadian Dollar",
            Currency::AUD => "Australian Dollar",
            Currency::NZD => "New Zealand Dollar",
            Currency::CNY => "Chinese Yuan",
            Currency::HKD => "Hong Kong Dollar",
            Currency::SGD => "Singapore Dollar",
            Currency::INR => "Indian Rupee",
            Currency::BRL => "Brazilian Real",
            Currency::MXN => "Mexican Peso",
            Currency::ZAR => "South African Rand",
            Currency::AED => "UAE Dirham",
            Currency::SAR => "Saudi Riyal",
            Currency::KRW => "South Korean Won",
            Currency::THB => "Thai Baht",
            Currency::MYR => "Malaysian Ringgit",
        }
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for Currency {
    type Err = CurrencyParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "USD" => Ok(Currency::USD),
            "EUR" => Ok(Currency::EUR),
            "GBP" => Ok(Currency::GBP),
            "JPY" => Ok(Currency::JPY),
            "CHF" => Ok(Currency::CHF),
            "CAD" => Ok(Currency::CAD),
            "AUD" => Ok(Currency::AUD),
            "NZD" => Ok(Currency::NZD),
            "CNY" => Ok(Currency::CNY),
            "HKD" => Ok(Currency::HKD),
            "SGD" => Ok(Currency::SGD),
            "INR" => Ok(Currency::INR),
            "BRL" => Ok(Currency::BRL),
            "MXN" => Ok(Currency::MXN),
            "ZAR" => Ok(Currency::ZAR),
            "AED" => Ok(Currency::AED),
            "SAR" => Ok(Currency::SAR),
            "KRW" => Ok(Currency::KRW),
            "THB" => Ok(Currency::THB),
            "MYR" => Ok(Currency::MYR),
            _ => Err(CurrencyParseError(s.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CurrencyParseError(String);

impl fmt::Display for CurrencyParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown currency code: {}", self.0)
    }
}

impl std::error::Error for CurrencyParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_currency_from_str() {
        assert_eq!(Currency::from_str("USD").unwrap(), Currency::USD);
        assert_eq!(Currency::from_str("usd").unwrap(), Currency::USD);
        assert_eq!(Currency::from_str("EUR").unwrap(), Currency::EUR);
        assert!(Currency::from_str("INVALID").is_err());
    }

    #[test]
    fn test_currency_display() {
        assert_eq!(Currency::USD.to_string(), "USD");
        assert_eq!(Currency::EUR.to_string(), "EUR");
    }

    #[test]
    fn test_currency_numeric_code() {
        assert_eq!(Currency::USD.numeric_code(), 840);
        assert_eq!(Currency::EUR.numeric_code(), 978);
        assert_eq!(Currency::GBP.numeric_code(), 826);
    }

    #[test]
    fn test_currency_decimal_places() {
        assert_eq!(Currency::USD.decimal_places(), 2);
        assert_eq!(Currency::JPY.decimal_places(), 0);
        assert_eq!(Currency::KRW.decimal_places(), 0);
    }

    #[test]
    fn test_currency_serialization() {
        let currency = Currency::USD;
        let json = serde_json::to_string(&currency).unwrap();
        assert_eq!(json, "\"USD\"");

        let deserialized: Currency = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Currency::USD);
    }
}
