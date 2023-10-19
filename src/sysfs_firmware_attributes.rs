// SPDX-License-Identifier: MIT OR Apache-2.0

use log::{error, info};
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::num::ParseIntError;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::{fs, io};
use strum::{AsRefStr, EnumString};

const POSSIBLE_VALUES_DELIMITER: &str = ";";
const ENUMERATION_VALUES_DELIMITER: &str = ":";
const SYSFS_END_LINE: &str = "\n";

const DEFAULT_INTEGER_MIN_VALUE: i32 = 0;
const DEFAULT_INTEGER_MAX_VALUE: i32 = i32::MAX;
const DEFAULT_INTEGER_SCALAR_INCREMENT: i32 = 1;

const DEFAULT_MIN_STRING_LENGTH: usize = 0;
const DEFAULT_MAX_STRING_LENGTH: usize = 128;

const DEFAULT_MIN_PASSWORD_LENGTH: usize = 0;
const DEFAULT_MAX_PASSWORD_LENGTH: usize = 128;

const PATH_ATTRIBUTES: &str = "attributes";
const PATH_AUTHENTICATIONS: &str = "authentication";

const ENUMERATION_LIST_ATTRIBUTES: &[&str] = &["BootOrder"];

const TYPE_ENUMERATION: &str = "enumeration";
const TYPE_INTEGER: &str = "integer";
const TYPE_STRING: &str = "string";
const TYPE_ORDERED_LIST: &str = "ordered-list";

const TYPE_ENUMERATION_LIST: &str = "enumeration-list";

const PATH_SYSFS_FIRMWARE_ATTRIBUTES: &str = "/sys/class/firmware-attributes/";

const PROPERTY_CURRENT_VALUE: &str = "current_value";
const PROPERTY_CURRENT_PASSWORD: &str = "current_password";
const PROPERTY_DEFAULT_VALUE: &str = "default_value";
const PROPERTY_DISPLAY_NAME: &str = "display_name";

#[derive(Debug)]
pub enum AttributeError {
    MissingFile(PathBuf),
    MissingDirectory(PathBuf),
    IOError(io::Error),
    ParseIntError(ParseIntError),
    UnsupportedAttributeType(String),
    VariantNotFount,
    InvalidRoot(PathBuf),
}

impl From<io::Error> for AttributeError {
    fn from(value: io::Error) -> Self {
        Self::IOError(value)
    }
}

impl From<ParseIntError> for AttributeError {
    fn from(value: ParseIntError) -> Self {
        Self::ParseIntError(value)
    }
}

impl From<strum::ParseError> for AttributeError {
    fn from(_: strum::ParseError) -> Self {
        Self::VariantNotFount
    }
}

impl Display for AttributeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Error for AttributeError {}

pub trait AttributeParser {
    type Attr: TryFrom<PathBuf, Error = AttributeError>;
    type Auth: TryFrom<PathBuf, Error = AttributeError>;

    fn attributes_names(path: &Path) -> Result<Vec<String>, AttributeError> {
        if is_firmware_attributes_root(path) {
            let attributes_path = path.join(PATH_ATTRIBUTES);
            directories_names(&attributes_path)
        } else {
            Err(AttributeError::InvalidRoot(path.to_path_buf()))
        }
    }

    fn authentications_names(path: &Path) -> Result<Vec<String>, AttributeError> {
        if is_firmware_attributes_root(path) {
            let authentications_path = path.join(PATH_AUTHENTICATIONS);
            directories_names(&authentications_path)
        } else {
            Err(AttributeError::InvalidRoot(path.to_path_buf()))
        }
    }

    fn attribute(path: &Path, attribute: &str) -> Result<Self::Attr, AttributeError> {
        if is_firmware_attributes_root(path) {
            path.join(PATH_ATTRIBUTES).join(attribute).try_into()
        } else {
            Err(AttributeError::InvalidRoot(path.to_path_buf()))
        }
    }

    fn authentication(path: &Path, authentication: &str) -> Result<Self::Auth, AttributeError> {
        if is_firmware_attributes_root(path) {
            path.join(PATH_AUTHENTICATIONS)
                .join(authentication)
                .try_into()
        } else {
            Err(AttributeError::InvalidRoot(path.to_path_buf()))
        }
    }

    fn pending_reboot(path: &Path) -> Result<bool, AttributeError> {
        if is_firmware_attributes_root(path) {
            Ok(read_attribute_property(&path.join(PATH_ATTRIBUTES), "pending_reboot")? == "1")
        } else {
            Err(AttributeError::InvalidRoot(path.to_path_buf()))
        }
    }
}

pub fn autodetect_root() -> Vec<PathBuf> {
    let root = PathBuf::from_str(PATH_SYSFS_FIRMWARE_ATTRIBUTES).unwrap();
    let mut list = Vec::new();
    if root.exists() {
        if is_firmware_attributes_root(&root) {
            list.push(root.clone());
        } else {
            if let Ok(dirs) = root.read_dir() {
                for dir in dirs {
                    if let Ok(dir) = dir {
                        if is_firmware_attributes_root(&dir.path()) {
                            list.push(dir.path());
                        }
                    }
                }
            }
        }
    }
    list
}

pub fn is_firmware_attributes_root(root: &Path) -> bool {
    root.join(PATH_AUTHENTICATIONS).exists() && root.join(PATH_ATTRIBUTES).exists()
}

fn directories_names(path: &Path) -> Result<Vec<String>, AttributeError> {
    if path.exists() && path.is_dir() {
        let mut result = Vec::<String>::new();
        for dir in fs::read_dir(path)? {
            let dir = dir?;
            if dir.metadata()?.is_dir() {
                result.push(dir.file_name().into_string().unwrap())
            }
        }
        Ok(result)
    } else {
        Err(AttributeError::MissingDirectory(path.to_path_buf()))
    }
}

pub trait ReadableAttribute {
    type Value;

    fn common_attribute(&self) -> &CommonAttribute<Self::Value>;
    fn current_value(&self) -> Result<Self::Value, AttributeError>;
}

pub trait WriteableAttribute: ReadableAttribute {
    fn write_current_value(
        &self,
        value: &<Self as ReadableAttribute>::Value,
    ) -> Result<(), AttributeError>;
}

#[derive(Debug, Clone)]
pub enum Attribute {
    Enumeration(EnumerationAttribute),
    Integer(IntegerAttribute),
    String(StringAttribute),
    OrderedList(OrderedListAttribute),
    EnumerationList(EnumerationListAttribute),
}

impl AttributeParser for Attribute {
    type Attr = Self;
    type Auth = Authentication;
}

impl TryFrom<PathBuf> for Attribute {
    type Error = AttributeError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        if value.exists() && value.is_dir() {
            match attribute_type(&value)?.as_str() {
                TYPE_ENUMERATION => Ok(Self::Enumeration(value.try_into()?)),
                TYPE_INTEGER => Ok(Self::Integer(value.try_into()?)),
                TYPE_STRING => Ok(Self::String(value.try_into()?)),
                TYPE_ORDERED_LIST => Ok(Self::OrderedList(value.try_into()?)),
                TYPE_ENUMERATION_LIST => Ok(Self::EnumerationList(value.try_into()?)),
                attribute_type => Err(AttributeError::UnsupportedAttributeType(
                    attribute_type.to_string(),
                )),
            }
        } else {
            Err(AttributeError::MissingDirectory(value.to_path_buf()))
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommonAttribute<T = String> {
    pub path: PathBuf,
    pub name: String,
    pub default_value: Option<T>,
    pub display_name: Option<String>,
    pub display_name_language_code: Option<String>,

    current_value_cache: Arc<Mutex<Option<T>>>,
}

impl TryFrom<PathBuf> for CommonAttribute {
    type Error = AttributeError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Ok(Self {
            name: attribute_name(&path),
            default_value: try_read_attribute_property(&path, PROPERTY_DEFAULT_VALUE)?,
            display_name: try_read_attribute_property(&path, PROPERTY_DISPLAY_NAME)?,
            display_name_language_code: try_read_attribute_property(
                &path,
                "display_name_language_code",
            )?,
            path,
            current_value_cache: Arc::new(Mutex::default()),
        })
    }
}

impl TryFrom<PathBuf> for CommonAttribute<Vec<String>> {
    type Error = AttributeError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        Ok(Self {
            name: attribute_name(&path),
            default_value: try_read_attribute_property(&path, PROPERTY_DEFAULT_VALUE)?.map(|s| {
                s.split(POSSIBLE_VALUES_DELIMITER)
                    .map(|s| s.to_string())
                    .collect()
            }),
            display_name: try_read_attribute_property(&path, PROPERTY_DISPLAY_NAME)?,
            display_name_language_code: try_read_attribute_property(
                &path,
                "display_name_language_code",
            )?,
            path,
            current_value_cache: Arc::new(Mutex::default()),
        })
    }
}

impl TryFrom<PathBuf> for CommonAttribute<i32> {
    type Error = AttributeError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let path = value.to_path_buf();
        let default_value = try_read_attribute_property(&path, PROPERTY_DEFAULT_VALUE)?;
        Ok(Self {
            name: attribute_name(&path),
            default_value: default_value
                .map(|v| i32::from_str(v.as_str()))
                .transpose()?,
            display_name: try_read_attribute_property(&path, PROPERTY_DISPLAY_NAME)?,
            display_name_language_code: try_read_attribute_property(
                &path,
                "display_name_language_code",
            )?,
            path,
            current_value_cache: Arc::new(Mutex::default()),
        })
    }
}

impl<T> CommonAttribute<T> {
    pub fn display_name(&self) -> &String {
        self.display_name.as_ref().unwrap_or(&self.name)
    }
}

impl<T: Clone> CommonAttribute<T> {
    fn current_value_cache_or<F: Fn() -> Result<T, AttributeError>>(
        &self,
        f: F,
    ) -> Result<T, AttributeError> {
        let mut lock = self.current_value_cache.lock().unwrap();
        if let Some(cache) = lock.as_ref() {
            Ok(cache.clone())
        } else {
            let value = f()?;
            lock.replace(value.clone());
            Ok(value)
        }
    }

    fn clear_current_value_cache(&self) {
        self.current_value_cache.lock().unwrap().take();
    }
}

fn attribute_name(root: &Path) -> String {
    root.file_name().unwrap().to_str().unwrap().to_string()
}

fn attribute_type(root: &Path) -> Result<String, AttributeError> {
    let attribute_name = attribute_name(root);
    let attribute_type = read_attribute_property(&root, "type")?;

    if attribute_type == TYPE_ENUMERATION
        && ENUMERATION_LIST_ATTRIBUTES.contains(&attribute_name.as_str())
    {
        Ok(TYPE_ENUMERATION_LIST.to_string())
    } else {
        Ok(attribute_type)
    }
}

fn read_attribute_property(root: &Path, property: &str) -> Result<String, AttributeError> {
    let path = root.join(property);
    if path.exists() {
        let string = fs::read_to_string(&path)?
            .trim_end_matches(SYSFS_END_LINE)
            .to_string();
        info!("Attribute read from path {:?} value {:?}", &path, string);
        Ok(string)
    } else {
        error!("Required Attribute not found at path {:?}", &path);
        Err(AttributeError::MissingFile(path))
    }
}

fn try_read_attribute_property(
    root: &Path,
    property: &str,
) -> Result<Option<String>, AttributeError> {
    let path = root.join(property);
    Ok(if path.exists() {
        let string = fs::read_to_string(&path)?
            .trim_end_matches(SYSFS_END_LINE)
            .to_string();
        info!("Attribute read from path {:?} value {:?}", &path, string);
        Some(string)
    } else {
        info!("Optional Attribute not found at path {:?}", &path);
        None
    })
}

fn write_attribute_property(
    root: &Path,
    property: &str,
    value: &str,
) -> Result<(), AttributeError> {
    let path = root.join(property);
    if path.exists() {
        let printable_value = if path.ends_with(PROPERTY_CURRENT_PASSWORD) {
            "<hidden>"
        } else {
            value
        };
        info!(
            "Write attribute path {:?} property {} value {}",
            path, property, printable_value
        );
        Ok(fs::write(path, value)?)
    } else {
        error!(
            "Cannot write attribute property. Attribute {:?} property {:?} not found",
            &path, property
        );
        Err(AttributeError::MissingFile(path))
    }
}

#[derive(Debug, Clone)]
pub struct EnumerationAttribute {
    pub common_attribute: CommonAttribute,
    pub possible_values: Vec<String>,
}

impl TryFrom<PathBuf> for EnumerationAttribute {
    type Error = AttributeError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let common_attribute = value.clone().try_into()?;
        let possible_values: Vec<String> = try_read_attribute_property(&value, "possible_values")?
            .map_or(Vec::new(), |s| {
                s.split(POSSIBLE_VALUES_DELIMITER)
                    .map(|s| s.to_string())
                    .collect()
            });
        Ok(Self {
            common_attribute,
            possible_values,
        })
    }
}

impl ReadableAttribute for EnumerationAttribute {
    type Value = String;

    fn common_attribute(&self) -> &CommonAttribute {
        &self.common_attribute
    }

    fn current_value(&self) -> Result<String, AttributeError> {
        Ok(self.common_attribute.current_value_cache_or(|| {
            read_attribute_property(&self.common_attribute.path, PROPERTY_CURRENT_VALUE)
        })?)
    }
}

impl WriteableAttribute for EnumerationAttribute {
    fn write_current_value(
        &self,
        value: &<Self as ReadableAttribute>::Value,
    ) -> Result<(), AttributeError> {
        let result =
            write_attribute_property(&self.common_attribute.path, PROPERTY_CURRENT_VALUE, value);
        self.common_attribute.clear_current_value_cache();
        result
    }
}

#[derive(Debug, Clone)]
pub struct OrderedListAttribute {
    pub common_attribute: CommonAttribute<Vec<String>>,
    pub elements: Vec<String>,
}

impl TryFrom<PathBuf> for OrderedListAttribute {
    type Error = AttributeError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let common_attribute = value.clone().try_into()?;
        let elements: Vec<String> = try_read_attribute_property(&value, "elements")?
            .or(try_read_attribute_property(&value, "possible_values")?)
            .map_or(Vec::new(), |s| {
                s.split(POSSIBLE_VALUES_DELIMITER)
                    .map(|s| s.to_string())
                    .collect()
            });
        Ok(Self {
            common_attribute,
            elements,
        })
    }
}

impl ReadableAttribute for OrderedListAttribute {
    type Value = Vec<String>;

    fn common_attribute(&self) -> &CommonAttribute<Self::Value> {
        &self.common_attribute
    }

    fn current_value(&self) -> Result<Vec<String>, AttributeError> {
        let value = self.common_attribute.current_value_cache_or(|| {
            let string =
                read_attribute_property(&self.common_attribute.path, PROPERTY_CURRENT_VALUE)?;
            Ok(string
                .split(POSSIBLE_VALUES_DELIMITER)
                .map(|s| s.to_string())
                .collect())
        });
        value
    }
}

impl WriteableAttribute for OrderedListAttribute {
    fn write_current_value(
        &self,
        value: &<Self as ReadableAttribute>::Value,
    ) -> Result<(), AttributeError> {
        let result = write_attribute_property(
            &self.common_attribute.path,
            PROPERTY_CURRENT_VALUE,
            &value.join(POSSIBLE_VALUES_DELIMITER),
        );
        self.common_attribute.clear_current_value_cache();
        result
    }
}

#[derive(Debug, Clone)]
pub struct EnumerationListAttribute {
    pub common_attribute: CommonAttribute<Vec<String>>,
    pub possible_values: Vec<String>,
}

impl TryFrom<PathBuf> for EnumerationListAttribute {
    type Error = AttributeError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let common_attribute = CommonAttribute {
            name: attribute_name(&value),
            default_value: try_read_attribute_property(&value, PROPERTY_DEFAULT_VALUE)?.map(|s| {
                s.split(ENUMERATION_VALUES_DELIMITER)
                    .map(|s| s.to_string())
                    .collect()
            }),
            display_name: try_read_attribute_property(&value, PROPERTY_DISPLAY_NAME)?,
            display_name_language_code: try_read_attribute_property(
                &value,
                "display_name_language_code",
            )?,
            path: value.clone(),
            current_value_cache: Arc::new(Mutex::default()),
        };
        let possible_values: Vec<String> = try_read_attribute_property(&value, "possible_values")?
            .map_or(Vec::new(), |s| {
                s.split(POSSIBLE_VALUES_DELIMITER)
                    .map(|s| s.to_string())
                    .collect()
            });
        Ok(Self {
            common_attribute,
            possible_values,
        })
    }
}

impl ReadableAttribute for EnumerationListAttribute {
    type Value = Vec<String>;

    fn common_attribute(&self) -> &CommonAttribute<Self::Value> {
        &self.common_attribute
    }

    fn current_value(&self) -> Result<Vec<String>, AttributeError> {
        let value = self.common_attribute.current_value_cache_or(|| {
            let string =
                read_attribute_property(&self.common_attribute.path, PROPERTY_CURRENT_VALUE)?;
            Ok(string
                .split(ENUMERATION_VALUES_DELIMITER)
                .map(|s| s.to_string())
                .collect())
        });
        value
    }
}

impl WriteableAttribute for EnumerationListAttribute {
    fn write_current_value(
        &self,
        value: &<Self as ReadableAttribute>::Value,
    ) -> Result<(), AttributeError> {
        let result = write_attribute_property(
            &self.common_attribute.path,
            PROPERTY_CURRENT_VALUE,
            &value.join(ENUMERATION_VALUES_DELIMITER),
        );
        self.common_attribute.clear_current_value_cache();
        result
    }
}

#[derive(Debug, Clone)]
pub struct IntegerAttribute {
    pub common_attribute: CommonAttribute<i32>,
    pub min_value: i32,
    pub max_value: i32,
    pub scalar_increment: i32,
}

impl TryFrom<PathBuf> for IntegerAttribute {
    type Error = AttributeError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let common_attribute = value.clone().try_into()?;
        let min_value = try_read_attribute_property(&value, "min_value")?
            .map(|s| i32::from_str(s.as_str()))
            .transpose()?
            .unwrap_or(DEFAULT_INTEGER_MIN_VALUE);
        let max_value = try_read_attribute_property(&value, "max_value")?
            .map(|s| i32::from_str(s.as_str()))
            .transpose()?
            .unwrap_or(DEFAULT_INTEGER_MAX_VALUE);
        let scalar_increment = try_read_attribute_property(&value, "scalar_increment")?
            .map(|s| i32::from_str(s.as_str()))
            .transpose()?
            .unwrap_or(DEFAULT_INTEGER_SCALAR_INCREMENT);
        Ok(Self {
            common_attribute,
            min_value,
            max_value,
            scalar_increment,
        })
    }
}

impl ReadableAttribute for IntegerAttribute {
    type Value = i32;

    fn common_attribute(&self) -> &CommonAttribute<Self::Value> {
        &self.common_attribute
    }

    fn current_value(&self) -> Result<i32, AttributeError> {
        self.common_attribute.current_value_cache_or(|| {
            let string =
                read_attribute_property(&self.common_attribute.path, PROPERTY_CURRENT_VALUE)?;
            Ok(i32::from_str(&string)?)
        })
    }
}

impl WriteableAttribute for IntegerAttribute {
    fn write_current_value(
        &self,
        value: &<Self as ReadableAttribute>::Value,
    ) -> Result<(), AttributeError> {
        let result = write_attribute_property(
            &self.common_attribute.path,
            PROPERTY_CURRENT_VALUE,
            &value.to_string(),
        );
        self.common_attribute.clear_current_value_cache();
        result
    }
}

#[derive(Debug, Clone)]
pub struct StringAttribute {
    pub common_attribute: CommonAttribute,
    pub max_length: usize,
    pub min_length: usize,
    pub hint: Option<String>,
}

impl TryFrom<PathBuf> for StringAttribute {
    type Error = AttributeError;

    fn try_from(value: PathBuf) -> Result<Self, Self::Error> {
        let common_attribute = value.clone().try_into()?;
        let min_length = try_read_attribute_property(&value, "min_length")?
            .map(|s| usize::from_str(s.as_str()))
            .transpose()?
            .unwrap_or(DEFAULT_MIN_STRING_LENGTH);
        let max_length = try_read_attribute_property(&value, "max_length")?
            .map(|s| usize::from_str(s.as_str()))
            .transpose()?
            .unwrap_or(DEFAULT_MAX_STRING_LENGTH);
        let hint = try_read_attribute_property(&value, "possible_values")?;
        Ok(Self {
            common_attribute,
            min_length,
            max_length,
            hint,
        })
    }
}

impl ReadableAttribute for StringAttribute {
    type Value = String;

    fn common_attribute(&self) -> &CommonAttribute<Self::Value> {
        &self.common_attribute
    }

    fn current_value(&self) -> Result<String, AttributeError> {
        self.common_attribute.current_value_cache_or(|| {
            read_attribute_property(&self.common_attribute.path, PROPERTY_CURRENT_VALUE)
        })
    }
}

impl WriteableAttribute for StringAttribute {
    fn write_current_value(
        &self,
        value: &<Self as ReadableAttribute>::Value,
    ) -> Result<(), AttributeError> {
        let result =
            write_attribute_property(&self.common_attribute.path, PROPERTY_CURRENT_VALUE, value);
        self.common_attribute.clear_current_value_cache();
        result
    }
}

#[derive(Debug, Clone)]
pub struct Authentication {
    pub path: PathBuf,
    pub login: String,
    pub is_enabled: bool,
    pub role: Role,
    pub mechanism: Mechanism,
    pub max_password_length: usize,
    pub min_password_length: usize,
}

impl TryFrom<PathBuf> for Authentication {
    type Error = AttributeError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        let login = path.file_name().unwrap().to_str().unwrap().to_string();
        let is_enabled = read_attribute_property(&path, "is_enabled")?.eq("1");
        let role = Role::from_str(read_attribute_property(&path, "role")?.as_str())?;
        let mechanism = Mechanism::from_str(read_attribute_property(&path, "mechanism")?.as_str())?;
        let min_password_length = try_read_attribute_property(&path, "min_password_length")?
            .map(|s| usize::from_str(s.as_str()))
            .transpose()?
            .unwrap_or(DEFAULT_MIN_PASSWORD_LENGTH);
        let max_password_length = try_read_attribute_property(&path, "max_password_length")?
            .map(|s| usize::from_str(s.as_str()))
            .transpose()?
            .unwrap_or(DEFAULT_MAX_PASSWORD_LENGTH);
        Ok(Self {
            path,
            login,
            is_enabled,
            role,
            mechanism,
            max_password_length,
            min_password_length,
        })
    }
}

impl Authentication {
    pub fn authenticate_with_password(&self, password: &str) -> Result<(), AttributeError> {
        write_attribute_property(&self.path, PROPERTY_CURRENT_PASSWORD, password)
    }
}

#[derive(Debug, EnumString, AsRefStr, Clone)]
pub enum Role {
    #[strum(serialize = "bios-admin")]
    BiosAdmin,
    #[strum(serialize = "power-on")]
    PowerOn,
    #[strum(serialize = "system-mgmt")]
    SystemMgmt,
    #[strum(serialize = "system")]
    System,
    #[strum(serialize = "hdd")]
    HDD, // Lenovo
    #[strum(serialize = "nvme")]
    NVMe, // Lenovo
    #[strum(serialize = "enhanced-bios-auth")]
    EnhancedBiosAuth, // HP
}

#[derive(Debug, EnumString, AsRefStr, Clone)]
pub enum Mechanism {
    #[strum(serialize = "password")]
    Password,
}
