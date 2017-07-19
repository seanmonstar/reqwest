use std::borrow::Cow;
use std::error;
use std::fmt;
use std::str;
use serde::ser::{self, Serialize};

use super::{MultipartRequest, MultipartField};

#[derive(Debug)]
pub enum Error {
    Custom(Cow<'static, str>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Custom(ref msg) => msg.fmt(f),
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Custom(ref msg) => msg,
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        match *self {
            Error::Custom(_) => None,
        }
    }
}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Error::Custom(format!("{}", msg).into())
    }
}

/// Converts serializable things to `MultipartRequest` if possible.
///
/// ```
/// # #[macro_use]
/// # extern crate serde_derive;
/// # extern crate reqwest;
/// use reqwest::{MultipartRequest, to_multipart};
///
/// #[derive(Serialize)]
/// struct Data {
///     name: &'static str,
///     age: u8
/// }
/// # fn main() {
/// let request: MultipartRequest = to_multipart(
///     Data {
///         name: "Sean",
///         age: 5
///     }
/// ).unwrap();
/// # }
/// ```
///
/// ```
/// use reqwest::{MultipartRequest, to_multipart};
///
/// let request: MultipartRequest = to_multipart(&[("name", "Sean"), ("age", "5")]).unwrap();
/// ```
///
/// ```
/// use std::collections::HashMap;
/// use reqwest::{MultipartRequest, to_multipart};
///
/// let mut map = HashMap::new();
/// map.insert("name", "Sean");
/// map.insert("age", "5");
///
/// let request = to_multipart(&map).unwrap();
/// ```
///
/// # Errors
/// Errors if the input cannot be converted.
pub fn to_multipart<T: Serialize>(data: T) -> Result<MultipartRequest, Error> {
    data.serialize(Serializer {} )
}

#[derive(Debug)]
struct Serializer;
impl ser::Serializer for Serializer {
    type Ok = MultipartRequest;
    type Error = Error;

    type SerializeSeq = SeqSerializer;
    type SerializeTuple = SeqSerializer;
    type SerializeTupleStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeMap = MapSerializer;
    type SerializeStruct = StructSerializer;
    type SerializeStructVariant = ser::Impossible<Self::Ok, Self::Error>;

    fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_u8(self, _: u8) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_str(self, _: &str) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_some<T: ?Sized>(
        self,
        value: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_unit_struct(
        self,
        _: &'static str
    ) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str
    ) -> Result<Self::Ok, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_newtype_struct<T: ?Sized>(
        self,
        _: &'static str,
        value: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
            value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
            Err(Error::top_level())
    }
    fn serialize_seq(
        self,
        _: Option<usize>
    ) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(SeqSerializer {pair_serializer: PairSerializer {current_key: None, output: MultipartRequest::new()}})
    }
    fn serialize_tuple(
        self,
        _: usize
    ) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(SeqSerializer {pair_serializer: PairSerializer {current_key: None, output: MultipartRequest::new()}})
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(Error::top_level())
    }
    fn serialize_map(
        self,
        _: Option<usize>
    ) -> Result<Self::SerializeMap, Self::Error> {
        Ok(MapSerializer {current_key: None, output: MultipartRequest::new()})
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(StructSerializer {output: MultipartRequest::new()})
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(Error::top_level())
    }
}

#[derive(Debug)]
struct SeqSerializer {
    pair_serializer: PairSerializer,
}
impl ser::SerializeSeq for SeqSerializer {
    type Ok = MultipartRequest;
    type Error = Error;

    fn serialize_element<T: ?Sized>(&mut self, value: &T ) -> Result<(), Self::Error>  where T: Serialize{
        value.serialize(&mut self.pair_serializer)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.pair_serializer.output)
    }
}
impl ser::SerializeTuple for SeqSerializer {
    type Ok = MultipartRequest;
    type Error = Error;

    fn serialize_element<T: ?Sized>(&mut self, value: &T ) -> Result<(), Self::Error>  where T: Serialize{
        value.serialize(&mut self.pair_serializer)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.pair_serializer.output)
    }
}

#[derive(Debug)]
struct StructSerializer {
    output: MultipartRequest,
}
impl ser::SerializeStruct for StructSerializer {
    type Ok = MultipartRequest;
    type Error = Error;
    fn serialize_field<T: ?Sized>(
        &mut self,
        key: &'static str,
        value: &T
    ) -> Result<(), Self::Error>
    where
        T: Serialize {
        self.output.fields(vec![MultipartField::param(key, value.serialize(ValueSerializer {})?.into_owned())]);
        Ok(())
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.output)
    }
}

#[derive(Debug)]
struct MapSerializer {
    current_key: Option<Cow<'static, str>>,
    output: MultipartRequest,
}
impl ser::SerializeMap for MapSerializer{
    type Ok = MultipartRequest;
    type Error = Error;
    fn serialize_key<T: ?Sized>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: Serialize {
        self.current_key = Some(key.serialize(KeySerializer {})?);
        Ok(())
    }
    fn serialize_value<T: ?Sized>(
        &mut self,
        value: &T
    ) -> Result<(), Self::Error>
    where
        T: Serialize {
        self.output.fields(vec![MultipartField::param(self.current_key.take().unwrap(), value.serialize(ValueSerializer {})?.into_owned())]);
        Ok(())
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(self.output)
    }
}

#[derive(Debug)]
struct PairSerializer {
    current_key: Option<Cow<'static, str>>,
    output: MultipartRequest,
}
impl<'a> ser::SerializeTuple for &'a mut PairSerializer {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized>(&mut self, value: &T ) -> Result<(), Self::Error>  where T: Serialize {
        self.current_key = match self.current_key.take() {
            Some(key) => {
                self.output.fields(vec![MultipartField::param(key, value.serialize(ValueSerializer {})?.into_owned())]);
                None
            }
            None => Some(value.serialize(KeySerializer {})?),
        };
        Ok(())
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        match self.current_key {
            Some(_) => Err(Error::missing_value()),
            None => Ok(()),
        }
    }
}
impl<'a> ser::Serializer for &'a mut PairSerializer {
	type Ok = ();
    type Error = Error;
    type SerializeSeq = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = Self;
    type SerializeTupleStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeMap = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = ser::Impossible<Self::Ok, Self::Error>;

	fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_u8(self, _: u8) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_str(self, _: &str) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_some<T: ?Sized>(
        self,
        _: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        Err(Error::not_tuple())
    }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_unit_struct(
        self,
        _: &'static str
    ) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str
    ) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_newtype_struct<T: ?Sized>(
        self,
        _: &'static str,
        _: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        Err(Error::not_tuple())
    }
    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        Err(Error::not_tuple())
    }
    fn serialize_seq(
        self,
        _: Option<usize>
    ) -> Result<Self::SerializeSeq, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_tuple(
        self,
        len: usize
    ) -> Result<Self::SerializeTuple, Self::Error> {
        if len == 2 {
            Ok(self)
        } else {
            Err(Error::tuple_is_not_pair())
        }
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_map(
        self,
        _: Option<usize>
    ) -> Result<Self::SerializeMap, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(Error::not_tuple())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(Error::not_tuple())
    }
}


#[derive(Debug)]
struct KeySerializer {}
impl ser::Serializer for KeySerializer {
	type Ok = Cow<'static, str>;
    type Error = Error;
    type SerializeSeq = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeMap = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = ser::Impossible<Self::Ok, Self::Error>;

	fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_u8(self, _: u8) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_char(self, _: char) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        // TODO: how to handle 'static str
        Ok(Cow::from(v.to_string()))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_some<T: ?Sized>(
        self,
        _: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        Err(Error::not_key())
    }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_unit_struct(
        self,
        _: &'static str
    ) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str
    ) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_newtype_struct<T: ?Sized>(
        self,
        _: &'static str,
        _: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        Err(Error::not_key())
    }
    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        Err(Error::not_key())
    }
    fn serialize_seq(
        self,
        _: Option<usize>
    ) -> Result<Self::SerializeSeq, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_tuple(
        self,
        _: usize
    ) -> Result<Self::SerializeTuple, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_map(
        self,
        _: Option<usize>
    ) -> Result<Self::SerializeMap, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(Error::not_key())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(Error::not_key())
    }
}

#[derive(Debug)]
struct ValueSerializer {}

impl ser::Serializer for ValueSerializer {
	type Ok = Cow<'static, [u8]>;
    type Error = Error;
    type SerializeSeq = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeMap = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStruct = ser::Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = ser::Impossible<Self::Ok, Self::Error>;

	fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(v.to_string().into_bytes()))
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        // TODO: how to handle &'static
        Ok(Cow::from(v.to_vec()))
    }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(Vec::new()))
    }
    fn serialize_some<T: ?Sized>(
        self,
        value: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        value.serialize(ValueSerializer {})
    }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(Cow::from(Vec::new()))
    }
    fn serialize_unit_struct(
        self,
        _: &'static str
    ) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_value())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str
    ) -> Result<Self::Ok, Self::Error> {
        Err(Error::not_value())
    }
    fn serialize_newtype_struct<T: ?Sized>(
        self,
        _: &'static str,
        value: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        value.serialize(ValueSerializer {})
    }
    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize {
        Err(Error::not_value())
    }
    fn serialize_seq(
        self,
        _: Option<usize>
    ) -> Result<Self::SerializeSeq, Self::Error> {
        Err(Error::not_value())
    }
    fn serialize_tuple(
        self,
        _: usize
    ) -> Result<Self::SerializeTuple, Self::Error> {
        Err(Error::not_value())
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(Error::not_value())
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(Error::not_value())
    }
    fn serialize_map(
        self,
        _: Option<usize>
    ) -> Result<Self::SerializeMap, Self::Error> {
        Err(Error::not_value())
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(Error::not_value())
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(Error::not_value())
    }
}

impl Error {
    fn top_level() -> Self {
        let msg = "top-level serializer supports only sequences, maps and structs";
        Error::Custom(msg.into())
    }

    fn not_key() -> Self {
        let msg = "value has to be convertible into a key";
        Error::Custom(msg.into())
    }

    fn not_value() -> Self {
        let msg = "value has to be convertible into [u8]";
        Error::Custom(msg.into())
    }

    fn missing_value() -> Self {
        let msg = "tried to serialize a key before completing the previous key";
        Error::Custom(msg.into())
    }

	fn not_tuple() -> Self {
        let msg = "tried to serialize sequence containing non tuples";
        Error::Custom(msg.into())
    }

	fn tuple_is_not_pair() -> Self {
        let msg = "tuple has to contain exactly 2 elements";
        Error::Custom(msg.into())
    }
}
