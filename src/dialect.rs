use std::fmt;
use std::str::FromStr;

use serde::Deserialize;

use crate::dialect_rules::mysql_rules;
use crate::rules::DialectRule;

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    #[default]
    Mysql,
}

impl Dialect {
    pub fn sql_dialect(&self) -> Box<dyn sqlparser::dialect::Dialect> {
        match self {
            Dialect::Mysql => Box::new(sqlparser::dialect::MySqlDialect {}),
        }
    }

    pub fn rules(&self) -> Vec<Box<dyn DialectRule>> {
        match self {
            Dialect::Mysql => mysql_rules(),
        }
    }
}

impl FromStr for Dialect {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mysql" => Ok(Dialect::Mysql),
            other => Err(format!("unknown dialect `{other}` — valid values: mysql")),
        }
    }
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Dialect::Mysql => write!(f, "mysql"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_mysql() {
        assert_eq!(Dialect::default(), Dialect::Mysql);
    }

    #[test]
    fn from_str_mysql() {
        assert_eq!("mysql".parse::<Dialect>().unwrap(), Dialect::Mysql);
        assert_eq!("MYSQL".parse::<Dialect>().unwrap(), Dialect::Mysql);
    }

    #[test]
    fn from_str_unknown_errors() {
        assert!("postgres".parse::<Dialect>().is_err());
    }

    #[test]
    fn display_mysql() {
        assert_eq!(Dialect::Mysql.to_string(), "mysql");
    }
}
