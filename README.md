# MStr

[![Latest Version]][crates-io]
[![Crates.io Downloads]][crates-io]
[![GitHub Stars]][github-com]

[![GitHub]][github-com]
[![Crates.io]][crates-io]
[![Docs.rs]][docs-rs]

[Latest Version]: https://img.shields.io/crates/v/mstr?label=version
[Crates.io Downloads]: https://img.shields.io/crates/d/mstr
[GitHub Stars]: https://img.shields.io/github/stars/Sky9x/mstr
[GitHub]: https://img.shields.io/badge/GitHub--white?style=social&logo=github
[Crates.io]: https://img.shields.io/badge/crates.io--white?style=social&logo=rust
[Docs.rs]: https://img.shields.io/badge/docs.rs--white?style=social&logo=docs.rs

[crates-io]: https://crates.io/crates/mstr
[github-com]: https://github.com/Sky9x/mstr
[docs-rs]: https://docs.rs/mstr

`MStr` is a 2-word, immutable version of `Cow<str>`.

`Cow<str>` is 4 words large, and not all applications need mutability.
Thus, `MStr` was born. It is just like `Cow<str>`, but is immutable and half the size.
The name is short for "maybe string", because it may be borrowed or owned (silly, I know).

You can think of it like an enum storing either a `&'a str` or a `Box<str>`.
However, such an enum would be 3 words (ptr/usize size) large,
because the single bit discriminant has to take up an entire word due to alignment.

But we can go smaller. If we can find somewhere to fold the discriminant bit
(indicating if we are borrowed or owned)
into the other fields (ptr and len), we can cut the size of this type by another word.

There are 3 potential places to put it:

The top bit of the pointer:  
Some architectures reserve the top bit for pointer tagging
(ARM actually reserves [the top 8 bits](https://en.wikichip.org/wiki/arm/tbi)).
However, not all architectures do this, so it would significantly reduce the portability of this library
(and I don't want to write any platform specific code), making this is a no-go.

The bottom bit of the pointer:  
This doesn't suffer from the portability issues of high bit tagging,
but requires that the pointer is [suitably aligned](https://en.wikipedia.org/wiki/Tagged_pointer)
(because if the address is a multiple of eg. 16, the bottom 4 bits must always be zero because Math™).
Unfortunately, strings in Rust are 1-byte aligned, so this won't work.

The top bit of len:  
This is the last viable option, so it better work (spoiler: it does).  
Rust limits all allocations to a maximum of `isize::MAX` bytes,
which means that the high bit is always zero, so we can use it for tagging. Yay!

So, this crate folds the discriminant bit into the high bit of the length field,
with `1` representing owned and `0` representing borrowed.
However, this is completely transparent to users of this crate,
so you don't need to worry it.

Happy smaller string-ing!


### Features

This crate has 1 feature (off by default):

- `serde`: Implement's `Serialize` & `Deserialize` for `MStr`.
Deserialization always returns an owned `MStr` (same behavior as `Cow`).


### No Std

This crate does not require the standard library (it is marked `#![no_std]`),
but it does require `alloc` (obviously).


## Contributing
Contributions on [GitHub](https://github.com/Sky9x/mstr) are welcome!
Feel free to open a PR or an issue for anything!


## License

This project is licensed under the MIT license OR the Apache 2.0 license, at your choice.
