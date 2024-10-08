use proc_macro2::{Span, TokenStream};
use quote::{quote_spanned, ToTokens as _, TokenStreamExt as _};
use syn::{parse::{discouraged::Speculative as _, Parse}, punctuated::{Pair, Punctuated}, spanned::Spanned as _, Ident, LitStr, Token};
use itertools::Itertools as _;
use std::{fmt::{Display, Debug}, str::FromStr};

/// Indicates that a type is data about Java Type signature, such as a *parameter type* or *return type*
pub trait SigType {
    /// The name of the function that maps from a `JValue` to a concrete type.
    /// This will always be a single *lowercase character*.
    /// Must be one of the single-letter functions found in [`JValueGen`][https://docs.rs/jni/latest/jni/objects/enum.JValueGen.html#method.l].
    fn sig_char(&self) -> Ident;
    /// The Type that is used in the signature of the method call. e.g. `"V"` or `"Ljava/lang/String;"`.
    /// It is the *uppercase [`SigType::sig_char`]*, and if it is `L` it is followed by the ClassPath and a `;`.
    fn sig_type(&self) -> LitStr;
}

/// A Type is an [`Ident`] of one of the following Variants in lowercase, or a [`Type::Object`].
///
/// Note that `Void` isn't a variant here.
/// Since [`Type`] is used by both [`Parameter`] and [`Return`],
/// and [`Paramter`] can't be void, the void variant was moved to [`Return`] (and by extension [`ResultType`]).
pub enum Type {
    JavaPrimitive { ident: Ident, ty: JavaPrimitive },
    RustPrimitive { ident: Ident, ty: RustPrimitive },
    Object(ClassPath),
}
impl Type {
    pub fn span(&self) -> Span {
        match self {
            Self::JavaPrimitive { ident, .. } | Self::RustPrimitive { ident, .. } => ident.span(),
            Self::Object(class) => class.span(),
        }
    }
}
impl SigType for Type {
    fn sig_char(&self) -> Ident {
        match self {
            Self::JavaPrimitive { ident, ty } => {
                let mut sig_char = ty.sig_char();
                sig_char.set_span(ident.span());
                sig_char
            },
            Self::RustPrimitive { ident, ty, .. } => {
                let mut sig_char = JavaPrimitive::from(*ty).sig_char();
                sig_char.set_span(ident.span());
                sig_char
            },
            Self::Object(class) => class.sig_char(),
        }
    }
    fn sig_type(&self) -> LitStr {
        match self {
            Self::JavaPrimitive { ident, ty } => {
                let mut sig_type = ty.sig_type();
                sig_type.set_span(ident.span());
                sig_type
            },
            Self::RustPrimitive { ident, ty } => {
                let mut sig_type = JavaPrimitive::from(*ty).sig_type();
                sig_type.set_span(ident.span());
                sig_type
            },
            Self::Object(class) => class.sig_type(),
        }
    }
}
impl Parse for Type {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let fork = input.fork();
        let ident = fork.parse::<Ident>()?;
        let ident_str = ident.to_string();
        match RustPrimitive::from_str(&ident_str).ok() {
            Some(ty) => {
                input.advance_to(&fork);
                Ok(Self::RustPrimitive { ident, ty })
            },
            // JavaPrimitive::Char will never be constructued here because the RustPrimitive takes priority
            None => match JavaPrimitive::from_str(&ident_str).ok() {
                Some(ty) => {
                    input.advance_to(&fork);
                    Ok(Self::JavaPrimitive { ident, ty })
                }
                None => return Ok(Self::Object(input.parse()?)),
            },
        }
    }
}
impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::JavaPrimitive { ty, .. } => ty.to_string(),
            // Convert form Rust to Java
            Self::RustPrimitive { ty, .. } => JavaPrimitive::from(*ty).to_string(),
            Self::Object(class) => format!("Object({})", class.to_jni_class_path()),
        };
        f.write_str(&s)
    }
}

/// Represents the path in the JVM of a *Java Class*, such as `java.lang.String`.
///
/// Parsed as [`Punctuated`] Tokens of [`Ident`]s and `Dot`s, disallowing trailing Dots.
/// Must have *at least one* package ident, so it will need at least 2 idents in total.
///
/// Example: `MyClass`, `java.lang.` are not allowed.
pub struct ClassPath {
    pub packages: Vec<(Ident, Token![.])>,
    /// The last Ident in the Punctuated list.
    pub class: Ident,
    pub nested_path: Option<NestedPath>
}
pub struct NestedPath {
    pub classes: Vec<(Ident, Token![$])>,
    pub final_class: Ident
}
impl ClassPath {
    /// Builds a [`TokenStream`] out of this path's items and returns its [`span`][TokenStream::span()].
    pub fn span(&self) -> Span {
        let mut tt = TokenStream::new();
        tt.append_all(self.packages.iter().map(|(t, p)| Pair::new(t, Some(p))));
        tt.append_all(self.class.to_token_stream());
        if let Some(nested_path) = &self.nested_path {
            tt.append_all(nested_path.classes.iter().map(|(t, p)| Pair::new(t, Some(p))));
            tt.append_all(nested_path.final_class.to_token_stream());
        }
        tt.span()
    }

    /// Parses the [`ClassPath`] with [`Parse::parse()`], but the final path *component* is a method name.
    /// 
    /// Returns the parsed [`ClassPath`] and the **method name**.
    pub fn parse_with_trailing_method(input: syn::parse::ParseStream) -> syn::Result<(Self, Ident)> {
        let mut path = Self::parse(input)?;

        let method_name = match &path.nested_path {
            // Parse the Dot and method name
            Some(_) => {
                input.parse::<Token![.]>()?;
                input.parse::<Ident>()?
            },
            // If the path had no Nested Classes (i.e. it was only separated by Dots)
            // the method name would have already been parsed, so take it out of the path.
            None => {
                // This implies that the path must be at least 3 components long: 2 for the path and 1 for the method name
                if path.packages.len() < 2 {
                    return Err(syn::Error::new(path.span(), "Java Class Path must have at least 1 package component (aside from the Class and method), such as `me.Class.method`"))
                }

                let method_name = path.class;
                path.class = path.packages.pop().unwrap().0;
                method_name
            }
        };

        Ok((path, method_name))
    }

    /// Converts the [`ClassPath`] to a string used by `JNI`, where each component is separated by a slash.
    /// e.g. `java/lang/String` or `me/author/Class$Nested`.
    #[allow(unstable_name_collisions)]
    pub fn to_jni_class_path(&self) -> String {
        self.to_string_helper("/", "$")
    }

    /// Helps create the string in [`Self::to_jni_class_path()`] and [`Self::fmt()`].
    /// 
    /// **sep** is the separator between the package and class component.
    /// **nested_sep** is the separator between the nested classes
    #[allow(unstable_name_collisions)]
    fn to_string_helper(&self, sep: &str, nested_sep: &str) -> String {
        let nested_classes = match &self.nested_path {
            Some(nested_path) => Box::new(
                // Put nested_sep between packages and nested classes
                Some(nested_sep.to_string()).into_iter()
                    .chain(nested_path.classes.iter()
                        .map(|(ident, _)| ident.to_string())
                        .chain(Some(nested_path.final_class.to_string()))
                        .intersperse(nested_sep.to_string())
                    )
            ) as Box<dyn Iterator<Item = String>>,
            None => Box::new(None.into_iter()) as Box<dyn Iterator<Item = String>>
        };

        self.packages.iter()
            .map(|(ident, _)| ident.to_string())
            .chain(Some(self.class.to_string()))
            .intersperse(sep.to_string())
            .chain(nested_classes)
            .collect::<String>()
    }
}
impl SigType for ClassPath {
    fn sig_char(&self) -> Ident {
        Ident::new("l", self.span())
    }
    fn sig_type(&self) -> LitStr {
        LitStr::new(&format!("L{};", self.to_jni_class_path()), self.span())
    }
}
impl Parse for ClassPath {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        /// Converts a [`Punctuated`] whose last element is [`None`] to a [`Vec`] of pairs.
        fn punctuated_to_pairs<T, P>(punctuated: Punctuated<T, P>) -> Vec<(T, P)> {
            punctuated.into_pairs()
                .into_iter()
                .map(|pair| pair.into_tuple())
                .filter_map(|pair| match pair.1 {
                    Some(punct) => Some((pair.0, punct)),
                    None => None, // Skips the last pair, because it will not have punct
                })
                .collect()
        }

        // Parse the dot-separated section `me.author.Class`
        let mut path = Punctuated::<Ident, Token![.]>::parse_separated_nonempty(input)?;
        // The class is the final component of the path
        let class = match path.pop() {
            Some(class) => class.into_value(),
            None => return Err(syn::Error::new(path.span(), "Java Class Path must not be empty"))
        };
        if path.is_empty() {
            return Err(syn::Error::new(path.span(), "Java Class Path must have more than one component, such as `me.author`"))
        }
        // Take the rest of the path components
        let packages = punctuated_to_pairs(path);

        // Path could have Nested classes, separated by `$`
        let nested_path = if input.parse::<Token![$]>().is_ok() {
            // Parse the Nested Class part of the path `Nested$Nested2`.
            let mut path = Punctuated::<Ident, Token![$]>::parse_separated_nonempty(input)?;
            // Is valid even if it containes only 1 component

            Some(NestedPath {
                // The class is the final component of the path
                final_class: match path.pop() {
                    Some(class) => class.into_value(),
                    None => return Err(syn::Error::new(path.span(), "Nested Class Path must not be empty"))
                },
                classes: punctuated_to_pairs(path)
            })
        } else {
            None
        };

        Ok(Self { class, packages, nested_path })
    }
}
// impl ToTokens for ClassPath {
//     /// Converts the path back to the same tokens it was obtained from: e.g. `java.lang.String`.
//     fn to_tokens(&self, tokens: &mut TokenStream) {
//         tokens.append_all(syn::parse_str::<TokenStream>(&self.to_string()).unwrap())
//     }
// }
impl Display for ClassPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string_helper(".", "."))
    }
}
impl Debug for ClassPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self, f)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RustPrimitive {
    Bool, Char,
    U8, U16, U32, U64,
    I8, I16, I32, I64,
    F32, F64
}
impl RustPrimitive {
    pub fn is_unsigned(self) -> bool {
        match self {
            Self::U8 | Self::U16 | Self::U32 | Self::U64 => true,
            _ => false,
        }
    }
    /// Handle special cases of primitives that must be converted from another type to get the target type.
    /// Returns [`None`] if the primitive doesn't need any conversion.
    /// 
    /// **value** is the tokens representing the value that will be *operated on*.
    pub fn special_case_conversion(self, value: TokenStream) -> Option<TokenStream> {
        if self.is_unsigned() {
            // Transmute to the unsigned type
            let target_ty = Ident::new(&self.to_string(), Span::call_site());
            Some(quote_spanned! {value.span()=> unsafe { ::std::mem::transmute::<_, #target_ty>(#value) } })
        } else if self == RustPrimitive::Char {
            // Decode UTF-16
            Some(quote_spanned! {value.span()=>
                char::decode_utf16(Some(#value))
                    .next().unwrap()
                    .unwrap_or(char::REPLACEMENT_CHARACTER)
            })
        } else {
            None
        }
    }
}
impl FromStr for RustPrimitive {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bool" => Ok(Self::Bool),
            "char" => Ok(Self::Char),
            "u8"   => Ok(Self::U8),
            "i8"   => Ok(Self::I8),
            "u16"  => Ok(Self::U16),
            "i16"  => Ok(Self::I16),
            "u32"  => Ok(Self::U32),
            "i32"  => Ok(Self::I32),
            "u64"  => Ok(Self::U64),
            "i64"  => Ok(Self::I64),
            "f32"  => Ok(Self::F32),
            "f64"  => Ok(Self::F64),
            _ => Err(()),
        }
    }
}
impl Display for RustPrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Bool => "bool",
            Self::Char => "char",
            Self::U8   => "u8",
            Self::I8   => "i8",
            Self::U16  => "u16",
            Self::I16  => "i16",
            Self::U32  => "u32",
            Self::I32  => "i32",
            Self::U64  => "u64",
            Self::I64  => "i64",
            Self::F32  => "f32",
            Self::F64  => "f64",
        })
    }
}
impl From<JavaPrimitive> for RustPrimitive {
    fn from(value: JavaPrimitive) -> Self {
        match value {
            JavaPrimitive::Boolean => RustPrimitive::Bool,
            JavaPrimitive::Char => RustPrimitive::Char,
            JavaPrimitive::Byte => RustPrimitive::I8,
            JavaPrimitive::Short => RustPrimitive::I16,
            JavaPrimitive::Int => RustPrimitive::I32,
            JavaPrimitive::Long => RustPrimitive::I64,
            JavaPrimitive::Float => RustPrimitive::F32,
            JavaPrimitive::Double => RustPrimitive::F64,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JavaPrimitive {
    Byte, Boolean, Char,
    Short, Int, Long,
    Float, Double
}
impl SigType for JavaPrimitive {
    fn sig_char(&self) -> Ident {
        let c = match self {
            Self::Byte    => 'b',
            Self::Boolean => 'z',
            Self::Char    => 'c',
            Self::Short   => 's',
            Self::Int     => 'i',
            Self::Long    => 'j',
            Self::Float   => 'f',
            Self::Double  => 'd',
        };
        Ident::new(&c.to_string(), Span::call_site())
    }
    fn sig_type(&self) -> LitStr {
        LitStr::new(
            self.sig_char()
                .to_string()
                .chars()
                .next()
                .unwrap()
                .to_uppercase()
                .collect::<String>()
                .as_str(),
            Span::call_site()
        )
    }
}
impl FromStr for JavaPrimitive {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "byte"    => Ok(Self::Byte),
            "boolean" => Ok(Self::Boolean),
            "char"    => Ok(Self::Char),
            "short"   => Ok(Self::Short),
            "int"     => Ok(Self::Int),
            "long"    => Ok(Self::Long),
            "float"   => Ok(Self::Float),
            "double"  => Ok(Self::Double),
            _ => Err(()),
        }
    }
}
impl Display for JavaPrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Byte    => "byte",
            Self::Boolean => "boolean",
            Self::Char    => "char",
            Self::Short   => "short",
            Self::Int     => "int",
            Self::Long    => "long",
            Self::Float   => "float",
            Self::Double  => "double",
        })
    }
}
impl From<RustPrimitive> for JavaPrimitive {
    fn from(value: RustPrimitive) -> Self {
        match value {
            RustPrimitive::Bool => JavaPrimitive::Boolean,
            RustPrimitive::Char => JavaPrimitive::Char,
            RustPrimitive::U8   => JavaPrimitive::Byte,
            RustPrimitive::I8   => JavaPrimitive::Byte,
            RustPrimitive::U16  => JavaPrimitive::Short,
            RustPrimitive::I16  => JavaPrimitive::Short,
            RustPrimitive::U32  => JavaPrimitive::Int,
            RustPrimitive::I32  => JavaPrimitive::Int,
            RustPrimitive::U64  => JavaPrimitive::Long,
            RustPrimitive::I64  => JavaPrimitive::Long,
            RustPrimitive::F32  => JavaPrimitive::Float,
            RustPrimitive::F64  => JavaPrimitive::Double,
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::parse::Parser;

    use super::*;

    #[test]
    fn class_path() {
        fn parse_str_with<P>(s: &str, parser: P) -> syn::Result<P::Output>
        where P: syn::parse::Parser {
            Parser::parse2(parser, syn::parse_str(s)?)
        }

        syn::parse_str::<ClassPath>("me.author.MyClass").unwrap();
        syn::parse_str::<ClassPath>("me.MyClass").unwrap();
        syn::parse_str::<ClassPath>("me.author.MyClass$Nested").unwrap();
        syn::parse_str::<ClassPath>("me.author.MyClass$Nested$Nested2").unwrap();
        parse_str_with("me.author.MyClass.method", ClassPath::parse_with_trailing_method).unwrap();
        parse_str_with("me.MyClass.method", ClassPath::parse_with_trailing_method).unwrap();
        parse_str_with("me.author.MyClass$Nested.method", ClassPath::parse_with_trailing_method).unwrap();
        parse_str_with("me.author.MyClass$Nested$Nested2.method", ClassPath::parse_with_trailing_method).unwrap();
        
        // Test errors
        syn::parse_str::<ClassPath>("").unwrap_err();
        syn::parse_str::<ClassPath>("me").unwrap_err();
        syn::parse_str::<ClassPath>("me.author.MyClass.").unwrap_err();
        syn::parse_str::<ClassPath>(".me.author.MyClass").unwrap_err();
        syn::parse_str::<ClassPath>("MyClass$Nested").unwrap_err();
        syn::parse_str::<ClassPath>("me.author.MyClass$").unwrap_err();
        syn::parse_str::<ClassPath>("me.author.MyClass$Nested$").unwrap_err();
        parse_str_with("MyClass.method", ClassPath::parse_with_trailing_method).unwrap_err();
        parse_str_with("MyClass$Nested.method", ClassPath::parse_with_trailing_method).unwrap_err();
    }
}