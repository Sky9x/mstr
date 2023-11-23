#![doc = include_str!("../README.md")]
#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use core::borrow::Borrow;
use core::cmp::Ordering;
use core::fmt::{Debug, Display, Formatter, Pointer};
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::ops::Deref;
use core::ptr::NonNull;
use core::{fmt, mem, ptr, str};

// the high bit of usize
// if set (1), MStr is owned
// if not (0), MStr is borrowed
const TAG: usize = 1 << (usize::BITS - 1);
// every bit except the tag bit
const MASK: usize = !TAG;

/// `MStr` is a 2-word, immutable version of `Cow<str>`.
///
/// See the [crate docs](crate) for more info.
pub struct MStr<'a> {
    ptr: NonNull<u8>,
    // if high bit (TAG) is set, we are owned
    // rust requires all allocations to be less than isize::MAX bytes,
    // so the top bit is never used and thus available for tagging
    len: usize,

    // use the lifetime (also makes it covariant)
    _marker1: PhantomData<&'a str>,
    // tell dropck that we might dealloc
    _marker2: PhantomData<Box<str>>,
}

unsafe impl Send for MStr<'_> {}
unsafe impl Sync for MStr<'_> {}

impl<'a> MStr<'a> {
    // -- Constructors --

    /// Creates a new `MStr<'a>` from an `&'a str`.
    ///
    /// The returned `MStr` is borrowed for the same lifetime as the input data.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// # use std::borrow::Cow;
    /// let s = String::from("foo");
    /// let mstr = MStr::new_borrowed(&s);
    ///
    /// assert!(mstr.is_borrowed());
    /// assert_eq!(mstr, s);
    /// assert_eq!(mstr.as_str(), "foo");
    /// assert_eq!(mstr.as_ptr(), s.as_ptr());
    /// assert!(matches!(mstr.into_cow(), Cow::Borrowed(_)));
    /// ```
    #[inline]
    #[must_use]
    pub const fn new_borrowed(s: &'a str) -> MStr<'a> {
        MStr::_new(s.as_ptr(), s.len(), false)
    }

    /// Creates a new `MStr` from owned data.
    /// The input type is anything that can be converted into a `Box<str>` (String, &str, etc).
    ///
    /// The returned `MStr` is owned.
    /// The lifetime can be chosen to be anything, including `'static`.
    ///
    /// If `s` is `Box<str>`, it will not reallocate.  
    /// If `s` is `String`, it [may reallocate](String::into_boxed_str) if there is excess capacity.  
    /// If `s` is `&str`, it will be copied to a new heap allocation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// # use std::borrow::Cow;
    /// let s = Box::<str>::from("foo");
    /// let ptr = s.as_ptr();
    /// let mstr = MStr::new_owned(s);
    ///
    /// assert!(mstr.is_owned());
    /// assert_eq!(mstr, "foo");
    /// assert_eq!(mstr.as_ptr(), ptr); // the allocation is reused
    /// assert!(matches!(mstr.into_cow(), Cow::Owned(_)));
    /// ```
    ///
    /// Passing a string slice makes an owned copy:
    /// ```rust
    /// # use mstr::MStr;
    /// let s = "bar";
    /// let ptr = s.as_ptr();
    /// let mstr = MStr::new_owned(s);
    ///
    /// assert!(mstr.is_owned());
    /// assert_eq!(mstr, s);
    /// assert_eq!(mstr, "bar");
    ///
    /// // a new allocation was created, and so the pointer are different
    /// assert_ne!(mstr.as_ptr(), s.as_ptr());
    /// ```
    #[must_use]
    pub fn new_owned(s: impl Into<Box<str>>) -> MStr<'a> {
        let s = s.into();

        let len = s.len();
        let ptr = Box::into_raw(s).cast::<u8>();

        MStr::_new(ptr, len, true)
    }

    /// Creates a new `MStr<'a>` from a `Cow<'a, str>`.
    ///
    /// The returned `MStr` will be borrowed if the cow is borrowed,
    /// and owned if the cow is owned.  
    /// If the cow is owned, and has excess capacity, it [may reallocate](String::into_boxed_str).
    ///
    /// # Examples
    ///
    /// Owned:
    /// ```rust
    /// # use mstr::MStr;
    /// # use std::borrow::Cow;
    /// let cow = Cow::Owned(String::from("foo"));
    /// let mstr = MStr::new_cow(cow);
    ///
    /// assert!(mstr.is_owned());
    /// assert_eq!(mstr, "foo");
    /// assert!(matches!(mstr.into_cow(), Cow::Owned(_)));
    /// ```
    ///
    /// Borrowed:
    /// ```rust
    /// # use mstr::MStr;
    /// # type Cow<'a> = std::borrow::Cow<'a, str>; // fix inference
    /// let s = String::from("bar");
    /// let cow = Cow::Borrowed(&s);
    /// let mstr = MStr::new_cow(cow);
    ///
    /// assert!(mstr.is_borrowed());
    /// assert_eq!(mstr, s);
    /// assert_eq!(mstr.as_ptr(), s.as_ptr());
    /// assert!(matches!(mstr.into_cow(), Cow::Borrowed(_)));
    /// ```
    ///
    /// Borrowed (static):
    /// ```rust
    /// # use mstr::MStr;
    /// # use std::borrow::Cow;
    /// let cow = Cow::Borrowed("qux");
    /// let mstr = MStr::new_cow(cow);
    ///
    /// assert!(mstr.is_borrowed());
    /// assert_eq!(mstr, "qux");
    /// assert!(matches!(mstr.into_cow(), Cow::Borrowed("qux")));
    /// ```
    #[inline]
    #[must_use]
    pub fn new_cow(s: Cow<'a, str>) -> MStr<'a> {
        match s {
            Cow::Borrowed(s) => MStr::new_borrowed(s),
            Cow::Owned(s) => MStr::new_owned(s),
        }
    }

    #[inline]
    #[must_use]
    const fn _new(ptr: *const u8, len: usize, tag: bool) -> MStr<'a> {
        MStr {
            // SAFETY: always comes from a valid string
            ptr: unsafe { NonNull::new_unchecked(ptr.cast_mut()) },
            len: if tag { len | TAG } else { len },

            _marker1: PhantomData,
            _marker2: PhantomData,
        }
    }

    // -- Accessors --

    /// Converts this `MStr` to a string slice.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let mstr = MStr::new_borrowed("foo");
    ///
    /// assert_eq!(mstr.as_str(), "foo");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &str {
        unsafe { &*self.as_str_ptr() }
    }

    /// Converts this `MStr` to a UTF-8 byte slice.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let mstr = MStr::new_borrowed("foo");
    ///
    /// assert_eq!(mstr.as_bytes(), b"foo");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }

    /// Converts this `MStr` into an owned `String`.
    /// This will consume `self`.
    ///
    /// If `self` is owned, the allocation will be reused.  
    /// If `self` is borrowed, it will be copied to the heap.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let mstr = MStr::new_borrowed("foo");
    /// let s: String = mstr.into_string();
    ///
    /// assert_eq!(s, "foo");
    /// ```
    ///
    /// Reuses owned allocation:
    /// ```rust
    /// # use mstr::MStr;
    /// let owned = Box::<str>::from("bar");
    /// let ptr = owned.as_ptr();
    /// let mstr = MStr::new_owned(owned);
    /// let s = mstr.into_string();
    ///
    /// assert_eq!(s, "bar");
    /// assert_eq!(s.as_ptr(), ptr);
    /// ```
    #[inline]
    #[must_use]
    pub fn into_string(self) -> String {
        self.into_cow().into_owned()
    }

    /// Converts this `MStr` into an owned `Box<str>`.
    /// This will consume `self`.
    ///
    /// If `self` is owned, the allocation will be reused.  
    /// If `self` is borrowed, it will be copied to the heap.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let mstr = MStr::new_borrowed("foo");
    /// let s: Box<str> = mstr.into_boxed();
    ///
    /// assert_eq!(&*s, "foo");
    /// ```
    ///
    /// Reuses owned allocation:
    /// ```rust
    /// # use mstr::MStr;
    /// let owned = Box::<str>::from("bar");
    /// let ptr = owned.as_ptr();
    /// let mstr = MStr::new_owned(owned);
    /// let s = mstr.into_boxed();
    ///
    /// assert_eq!(&*s, "bar");
    /// assert_eq!(s.as_ptr(), ptr);
    /// ```
    #[inline]
    #[must_use]
    pub fn into_boxed(self) -> Box<str> {
        self.into_string().into_boxed_str()
    }

    /// Converts this `MStr<'a>` into a `Cow<'a, str>`.
    /// This will consume `self`.
    ///
    /// The returned cow will be owned if `self` is owned, and borrowed if `self` is borrowed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// # use std::borrow::Cow;
    /// let borrowed = MStr::new_borrowed("foo");
    /// let owned = MStr::new_owned("bar");
    ///
    /// assert!(matches!(borrowed.into_cow(), Cow::Borrowed("foo")));
    /// assert!(matches!(owned.into_cow(), Cow::Owned(_)));
    /// ```
    #[must_use]
    pub fn into_cow(self) -> Cow<'a, str> {
        let ptr = self.as_str_ptr();
        let is_owned = self.is_owned();
        mem::forget(self);

        if is_owned {
            let b = unsafe { Box::from_raw(ptr.cast_mut()) };
            Cow::Owned(b.into_string())
        } else {
            Cow::Borrowed(unsafe { &*ptr })
        }
    }

    /// Checks if this `MStr` is owned.
    ///
    /// The result of this function is mutually exclusive with [`is_borrowed`](MStr::is_borrowed).
    /// Exactly one of `is_borrowed` and `is_owned` will be true for every `MStr`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let mstr = MStr::new_owned("bar");
    ///
    /// assert!(mstr.is_owned());
    /// assert!(!mstr.is_borrowed());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_owned(&self) -> bool {
        self.len & TAG == TAG
    }

    /// Checks if this `MStr` is borrowed.
    ///
    /// The result of this function is mutually exclusive with [`is_owned`](MStr::is_owned).
    /// Exactly one of `is_borrowed` and `is_owned` will be true for every `MStr`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let mstr = MStr::new_borrowed("bar");
    ///
    /// assert!(mstr.is_borrowed());
    /// assert!(!mstr.is_owned());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_borrowed(&self) -> bool {
        self.len & TAG == 0
    }

    /// Gets the length of the underlying string slice.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let mstr = MStr::new_borrowed("12345");
    ///
    /// assert_eq!(mstr.len(), 5);
    /// ```
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len & MASK
    }

    /// Checks if the underlying string slice is empty (length of 0)
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let empty = MStr::new_borrowed("");
    /// let mstr = MStr::new_borrowed("foo");
    ///
    /// assert!(empty.is_empty());
    /// assert!(!mstr.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Gets a pointer (`*const u8`) to the underlying slice's buffer.
    ///
    /// Do **NOT** use the returned pointer mutably, as `self` may be borrowed.
    ///
    /// Use [`as_str_ptr`](MStr::as_str_ptr) if you want a `*const str` instead.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let s = "foo";
    /// let mstr = MStr::new_borrowed(s);
    ///
    /// assert_eq!(mstr.as_ptr(), s.as_ptr());
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Gets a pointer (`*const str`) to the underlying slice's buffer.
    ///
    /// Do **NOT** use the returned pointer mutably, as `self` may be borrowed.
    ///
    /// Use [`as_ptr`](MStr::as_ptr) if you want a `*const u8` instead.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let s = "foo";
    /// let mstr = MStr::new_borrowed(s);
    ///
    /// assert_eq!(mstr.as_str_ptr(), s as *const str);
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_str_ptr(&self) -> *const str {
        ptr::slice_from_raw_parts::<u8>(self.as_ptr(), self.len()) as *const str
    }
}

// ===== Trait Impls =====

impl Clone for MStr<'_> {
    /// Clones this `MStr`.
    ///
    /// The returned `MStr` will be owned if `self` is owned, and borrowed if `self` is borrowed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let mstr = MStr::new_owned("foo");
    /// let mstr2 = mstr.clone();
    ///
    /// assert_eq!(mstr, mstr2);
    /// ```
    ///
    /// Borrowed/Owned is preserved:
    /// ```rust
    /// # use mstr::MStr;
    /// let borrowed = MStr::new_borrowed("bar");
    /// let owned = MStr::new_owned("qux");
    ///
    /// assert!(borrowed.clone().is_borrowed());
    /// assert!(owned.clone().is_owned());
    /// ```
    fn clone(&self) -> Self {
        if self.is_borrowed() {
            MStr::_new(self.as_ptr(), self.len(), false)
        } else {
            MStr::new_owned(self.as_str())
        }
    }
}

impl Drop for MStr<'_> {
    fn drop(&mut self) {
        if self.is_owned() {
            let b = unsafe { Box::from_raw(self.as_str_ptr().cast_mut()) };
            drop(b);
        }
    }
}

// -- Default --

impl Default for MStr<'_> {
    /// Creates a new, empty, borrowed `MStr`.
    ///
    /// The returned `MStr` can have any lifetime.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use mstr::MStr;
    /// let default = MStr::default();
    ///
    /// assert_eq!(default, "");
    /// assert!(default.is_empty());
    /// assert!(default.is_borrowed());
    /// ```
    fn default() -> Self {
        // a dangling (suitably aligned) slice of length 0 is always valid
        MStr::_new(NonNull::<u8>::dangling().as_ptr(), 0, false)
    }
}

// -- Format --

impl Debug for MStr<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self.as_str(), f)
    }
}

impl Display for MStr<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self.as_str(), f)
    }
}

impl Pointer for MStr<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Pointer::fmt(&self.as_str_ptr(), f)
    }
}

// -- Convert From --

impl<'a> From<&'a str> for MStr<'a> {
    fn from(value: &'a str) -> Self {
        MStr::new_borrowed(value)
    }
}

impl<'a> From<&'a mut str> for MStr<'a> {
    fn from(value: &'a mut str) -> Self {
        MStr::new_borrowed(value)
    }
}

impl<'a> From<Cow<'a, str>> for MStr<'a> {
    fn from(value: Cow<'a, str>) -> Self {
        MStr::new_cow(value)
    }
}

impl From<String> for MStr<'_> {
    fn from(value: String) -> Self {
        MStr::new_owned(value)
    }
}

impl From<Box<str>> for MStr<'_> {
    fn from(value: Box<str>) -> Self {
        MStr::new_owned(value)
    }
}

// -- Convert To --

impl<'a> From<MStr<'a>> for Cow<'a, str> {
    fn from(value: MStr<'a>) -> Self {
        value.into_cow()
    }
}

impl From<MStr<'_>> for String {
    fn from(value: MStr<'_>) -> Self {
        value.into_string()
    }
}

impl From<MStr<'_>> for Box<str> {
    fn from(value: MStr<'_>) -> Self {
        value.into_boxed()
    }
}

// -- Convert Ref --

impl Deref for MStr<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for MStr<'_> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for MStr<'_> {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Borrow<str> for MStr<'_> {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

// no Borrow<[u8]> because str/String don't implement it
// (because the Hash impls are different)

// -- Hash --

impl Hash for MStr<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Hash::hash(self.as_str(), state)
    }
}

// -- [Partial]Eq --

impl Eq for MStr<'_> {}

impl PartialEq for MStr<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

// str

impl PartialEq<str> for MStr<'_> {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<MStr<'_>> for str {
    fn eq(&self, other: &MStr<'_>) -> bool {
        self == other.as_str()
    }
}

// &str

impl PartialEq<&str> for MStr<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<MStr<'_>> for &str {
    fn eq(&self, other: &MStr<'_>) -> bool {
        *self == other.as_str()
    }
}

// String

impl PartialEq<String> for MStr<'_> {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<MStr<'_>> for String {
    fn eq(&self, other: &MStr<'_>) -> bool {
        self == other.as_str()
    }
}

// Box<str>

impl PartialEq<Box<str>> for MStr<'_> {
    fn eq(&self, other: &Box<str>) -> bool {
        self.as_str() == &**other
    }
}

impl PartialEq<MStr<'_>> for Box<str> {
    fn eq(&self, other: &MStr<'_>) -> bool {
        &**self == other.as_str()
    }
}

// -- [Partial]Ord --

impl Ord for MStr<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl PartialOrd for MStr<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialOrd<str> for MStr<'_> {
    fn partial_cmp(&self, other: &str) -> Option<Ordering> {
        self.as_str().partial_cmp(other)
    }
}

impl PartialOrd<MStr<'_>> for str {
    fn partial_cmp(&self, other: &MStr<'_>) -> Option<Ordering> {
        self.partial_cmp(other.as_str())
    }
}

// ===== serde =====

#[cfg(feature = "serde")]
mod serde_impls {
    use super::*;
    use serde::de::{Error, Visitor};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    // -- Serialize --

    impl Serialize for MStr<'_> {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_str(self.as_str())
        }
    }

    // -- Deserialize --

    struct MStrVisitor;

    impl Visitor<'_> for MStrVisitor {
        type Value = MStr<'static>;

        fn expecting(&self, f: &mut Formatter) -> fmt::Result {
            f.write_str("a string")
        }

        fn visit_str<E: Error>(self, s: &str) -> Result<Self::Value, E> {
            Ok(MStr::new_owned(s))
        }

        fn visit_string<E: Error>(self, s: String) -> Result<Self::Value, E> {
            Ok(MStr::new_owned(s))
        }
    }

    impl<'de, 'a> Deserialize<'de> for MStr<'a> {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            d.deserialize_string(MStrVisitor)
        }
    }

    // -- Unit Tests --

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde::de::DeserializeOwned;
        use serde_json::json;
        use serde_test::{assert_tokens, Token};

        #[test]
        fn basic() {
            assert_tokens(&MStr::from("roar"), &[Token::BorrowedStr("roar")]);
            assert_tokens(&MStr::from("honk"), &[Token::Str("honk")]);
            assert_tokens(&MStr::from("quack"), &[Token::String("quack")]);
        }

        #[test]
        fn always_de_owned() {
            let not_static = String::from("\"frogs <3\"");

            let s: MStr<'static> = serde_json::from_str(&not_static).unwrap();

            assert_eq!(s, "frogs <3");
            assert!(s.is_owned());
        }

        #[test]
        fn de_value() {
            let s: MStr<'static> =
                serde_json::from_value(json!("i like frogs can you tell")).unwrap();

            assert_eq!(s, "i like frogs can you tell");
            assert!(s.is_owned());
        }

        #[test]
        fn assert_deserialize_owned() {
            fn assert_deserialize_owned<T: DeserializeOwned>() {}

            assert_deserialize_owned::<MStr>();
            assert_deserialize_owned::<MStr<'static>>();
        }
    }
}

// ===== Unit Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    // fix inference
    type Cow<'a> = alloc::borrow::Cow<'a, str>;

    #[test]
    fn correct_repr() {
        assert!(MStr::new_borrowed("abc").is_borrowed());
        assert!(!MStr::new_borrowed("abc").is_owned());

        assert!(MStr::new_owned("123").is_owned());
        assert!(!MStr::new_owned("123").is_borrowed());
    }

    #[test]
    fn empty() {
        assert!(MStr::new_borrowed("").is_empty());
        assert!(MStr::new_owned("").is_empty());
        assert!(MStr::default().is_empty());

        assert_eq!(MStr::new_borrowed("").len(), 0);
        assert_eq!(MStr::new_owned("").len(), 0);
        assert_eq!(MStr::default().len(), 0);
    }

    #[test]
    fn len() {
        assert_eq!(MStr::new_borrowed("12345").len(), 5);
        assert_eq!(MStr::new_owned("12345").len(), 5);
    }

    #[test]
    fn borrowed_stays_borrowed() {
        let s = "1234";
        let mstr = MStr::new_borrowed(s);

        assert_eq!(mstr, s);
        assert_eq!(mstr.as_str(), s);

        assert_eq!(mstr.as_ptr(), s.as_ptr());
        assert_eq!(mstr.as_str().as_ptr(), s.as_ptr());
        assert_eq!(mstr.as_str_ptr(), s as *const str);

        let clone = mstr.clone();

        assert!(clone.is_borrowed());
        assert!(!clone.is_owned());

        assert_eq!(mstr, clone);
        assert_eq!(mstr.as_ptr(), clone.as_ptr());
        assert_eq!(mstr.as_str_ptr(), clone.as_str_ptr());
    }

    #[test]
    fn into_cow() {
        assert_eq!(MStr::new_borrowed("meow").into_cow(), Cow::Borrowed("meow"));
        assert_eq!(
            MStr::new_owned("woof").into_cow(),
            Cow::Owned(String::from("woof"))
        );
        assert_eq!(
            MStr::new_cow(Cow::Borrowed("purr")).into_cow(),
            Cow::Borrowed("purr")
        );
        assert_eq!(
            MStr::new_cow(Cow::Owned("bark".into())).into_cow(),
            Cow::Owned(String::from("bark"))
        );
    }

    #[test]
    fn roundtrip() {
        assert_eq!(MStr::new_borrowed("foo").into_string(), String::from("foo"));
        assert_eq!(MStr::new_owned("bar").into_string(), String::from("bar"));
    }

    #[test]
    fn roundtrip_string_ptr() {
        let s = String::from("quack");
        let ptr = s.as_ptr();
        let mstr = MStr::new_owned(s);

        assert_eq!(mstr, "quack");
        assert_eq!(mstr.as_ptr(), ptr);
        assert_eq!(mstr.into_string().as_ptr(), ptr);
    }

    #[test]
    fn owned_clone() {
        let mstr = MStr::new_owned("quack");
        let mstr2 = mstr.clone();

        assert!(mstr.is_owned());
        assert!(mstr2.is_owned());
        assert!(!mstr2.is_borrowed());

        assert_eq!(mstr, mstr2);
        assert_ne!(mstr.as_ptr(), mstr2.as_ptr());
        assert_ne!(mstr.as_str_ptr(), mstr2.as_str_ptr());
    }

    #[test]
    fn static_lt() {
        let owned: MStr<'static> = MStr::new_owned("abc");
        let borrowed: MStr<'static> = MStr::new_borrowed("abc");

        assert_eq!(owned, borrowed);
    }

    #[test]
    fn covariant_lt() {
        fn same_lt<'a>(a: &MStr<'a>, b: &MStr<'a>, s: &'a str) {
            assert_eq!(a, b);
            assert_eq!(a, s);
            assert_eq!(b, s);
        }

        let st1: MStr<'static> = MStr::new_borrowed("oink");
        let st2: MStr<'static> = MStr::new_owned("oink");

        same_lt(&st1, &st2, "oink");

        let s = String::from("oink");
        let ms = MStr::new_borrowed(&s);

        same_lt(&st1, &ms, &s);

        //

        fn coerce_any_lt_owned<'a>() -> MStr<'a> {
            MStr::new_owned("abc")
        }
        assert_eq!(coerce_any_lt_owned(), "abc");

        fn coerce_any_lt_borrowed<'a>() -> MStr<'a> {
            MStr::new_borrowed("123")
        }
        assert_eq!(coerce_any_lt_borrowed(), "123");
    }

    #[test]
    fn assert_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<MStr>();
        assert_send_sync::<MStr<'static>>();
    }
}
