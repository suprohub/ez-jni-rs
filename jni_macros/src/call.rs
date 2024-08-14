use std::fmt::Display;

use either::Either;
use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned, ToTokens, TokenStreamExt};
use syn::{braced, bracketed, parenthesized, parse::{discouraged::Speculative, Parse}, punctuated::{Pair, Punctuated}, spanned::Spanned, Expr, Ident, LitInt, LitStr, Token};

/// Define a JNI call to a Java method with parameters with expected types, and an expected return type.
/// 
/// See [`crate::call`] for an example.
pub struct MethodCall {
    pub call_type: Either<StaticMethod, ObjectMethod>,
    pub method_name: Ident,
    pub parameters: Punctuated<Parameter, Token![,]>,
    pub return_type: Return,
}
impl Parse for MethodCall {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self {
            call_type: input.parse::<StaticMethod>()
                .map(|i| Either::Left(i))
                .or_else(|_| input.parse::<ObjectMethod>().map(|i| Either::Right(i)))?,
            method_name: {
                input.parse()?
            },
            parameters: {
                let arg_tokens;
                parenthesized!(arg_tokens in input);
                Punctuated::parse_terminated(&arg_tokens)?
            },
            return_type: {
                input.parse::<Token![->]>()?;
                input.parse()?
            }
        })
    }
}

/// The call is for a static method.
/// ```
/// method_call!(static java.lang.String::methodName ...);
/// ```
pub struct StaticMethod(pub ClassPath);
impl Parse for StaticMethod {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        input.parse::<Token![static]>()?;
        let path = input.parse()?;
        // TODO: use Dot to separate Class path and method name instead
        input.parse::<Token![::]>()?;
        Ok(Self(path))
    }
}

/// The call is for a Method of an existing Object, stored in a variable.
/// If the object is more than an Ident, it must be enclosed in `parenthesis` or `braces`.
/// e.g. `object.methodName(...)` or `(something.object).methodName(...)`.
pub struct ObjectMethod(pub TokenStream);
impl Parse for ObjectMethod {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let expr = if input.peek(syn::token::Paren) {
            let inner;
            parenthesized!(inner in input);
            inner.parse::<TokenStream>()?
        } else if input.peek(syn::token::Brace) {
            let inner;
            braced!(inner in input);
            inner.parse::<TokenStream>()?
        } else {
            input.parse::<Ident>()?.to_token_stream()
        };
        input.parse::<Token![.]>()?;
        Ok(Self(expr))
    }
}

/// Represents the path in the jvm of a Java Class, such as `java.lang.String`.
/// 
/// Parsed as [`Punctuated`] Tokens of [`Ident`]s and `Dot`s, disallowing trailing Dots.
pub struct ClassPath {
    pub packages: Vec<(Ident, Token![.])>,
    /// The last Ident in the Punctuated list.
    pub class: Ident
}
impl ClassPath {
    /// Returns the Type that is used in the signature of the method call. e.g. `V` or `Ljava/lang/String;`
    pub fn sig_type(&self) -> String { format!("L{self};") }
    pub fn span(&self) -> Span {
        let mut tt = TokenStream::new();
        tt.append_all(self.packages.iter().map(|(t, p)| Pair::new(t, Some(p))));
        tt.append_all(self.class.to_token_stream());
        tt.span()
    }
}
impl Parse for ClassPath {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let path = Punctuated::<Ident, Token![.]>::parse_separated_nonempty(input)?;
        
        Ok(Self {
            class: path.last().unwrap().clone(),
            packages: path.into_pairs()
                .into_iter()
                .map(|pair| pair.into_tuple())
                .filter_map(|pair| match pair.1 {
                    Some(punct) => Some((pair.0, punct)),
                    None => None
                })
                .collect(),
        })
    }
}
impl Display for ClassPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&
            Itertools::intersperse(
                self.packages.iter()
                    .map(|pair| pair.0.to_string())
                    .chain(Some(self.class.to_string())),
                "/".to_string()
            ).collect::<String>()
        )
    }
}

/// A Type is an [`Ident`] of one of the following Variants in lowercase, or a [`Type::Object`]
pub enum Type {
    Void(Ident),
    Byte(Ident), Bool(Ident), Char(Ident),
    Short(Ident), Int(Ident), Long(Ident),
    Float(Ident), Double(Ident),
    Object(ClassPath)
}
impl Type {
    /// Returns the character/letter (lowercase) that is used to convert from JValue to a concrete type.
    pub fn sig_char(&self) -> char {
        match self {
            Self::Void(_)   => 'v',
            Self::Byte(_)   => 'b',
            Self::Bool(_)   => 'z',
            Self::Char(_)   => 'c',
            Self::Short(_)  => 's',
            Self::Int(_)    => 'i',
            Self::Long(_)   => 'j',
            Self::Float(_)  => 'f',
            Self::Double(_) => 'd',
            Self::Object(_) => 'l'
        }
    }
    /// Returns the Type that is used in the signature of the method call. e.g. `V` or `Ljava/lang/String;`
    pub fn sig_type(&self) -> String {
        match self {
            Self::Object(class) => class.sig_type(),
            _ => self.sig_char().to_uppercase().to_string()
        }
    }
    fn is_void(&self) -> bool {
        match self {
            Self::Void(_) => true,
            _ => false,
        }
    }
    pub fn span(&self) -> Span {
        match self {
            Self::Void(ident) | Self::Byte(ident) | Self::Bool(ident)
            | Self::Char(ident) | Self::Short(ident)  | Self::Int(ident)   
            | Self::Long(ident) | Self::Float(ident)
            | Self::Double(ident) => ident.span(),
            Self::Object(class) => class.span(),
        }
    }
}
impl Parse for Type {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let fork = input.fork();
        let ident = fork.parse::<Ident>()?;
        let variant = match ident.to_string().as_str() {
            "void"   => Self::Void(ident),
            "byte"   => Self::Byte(ident),
            "bool"   => Self::Bool(ident),
            "char"   => Self::Char(ident),
            "short"  => Self::Short(ident),
            "int"    => Self::Int(ident),
            "long"   => Self::Long(ident),
            "float"  => Self::Float(ident),
            "double" => Self::Double(ident),
            // Return early to prevent advancing the original ParseStream to the fork
            _ => return Ok(Self::Object(input.parse()?))
        };
        input.advance_to(&fork);
        
        Ok(variant)
    }
}
impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Void(_)   => "void",
            Self::Byte(_)   => "byte",
            Self::Bool(_)   => "bool",
            Self::Char(_)   => "char",
            Self::Short(_)  => "short",
            Self::Int(_)    => "int",
            Self::Long(_)   => "long",
            Self::Float(_)  => "float",
            Self::Double(_) => "double",
            Self::Object(class) => &format!("Object({class})")
        };
        f.write_str(s)
    }
}

/// Parameter to a JNI method call, such as `int(value)` or `java.lang.String(value)`.
pub enum Parameter {
    /// `int(value)` or `java.lang.String(value)`
    Single {
        ty: Type,
        value: TokenStream,
    },
    /// `[int](value)` or `[java.lang.String](value)`
    Array {
        ty: Type,
        value: TokenStream,
    },
    /// `[int]([value1, value2])` or `[java.lang.String]([value1, value2])`
    ArrayLiteral {
        ty: Type,
        array: Punctuated<Expr, Token![,]>
    }
}
impl Parameter {
    /// Returns the [`Type`] of the parameter.
    pub fn ty(&self) -> &Type {
        match self {
            Self::Single { ty, .. } => ty,
            Self::Array { ty, .. } => ty,
            Self::ArrayLiteral { ty, .. } => ty,
        }
    }
    /// Returns whether the parameter represents an Array (literal or by value).
    pub fn is_array(&self) -> bool {
        match self {
            Self::Single { .. } => false,
            Self::Array { .. } | Self::ArrayLiteral { .. } => true
        }
    }
}
impl Parse for Parameter {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Check if the Type is wrapped in Brackets (Array)
        Ok(if input.lookahead1().peek(syn::token::Bracket) {
            let ty = {
                let inner;
                bracketed!(inner in input);
                inner.parse::<Type>()?
            };
            if ty.is_void() {
                return Err(syn::Error::new(ty.span(), "Parameters can't have type Void"))
            }
            let value_tokens;
            parenthesized!(value_tokens in input);
            // Check if the Value is wrapped in Brackets (ArrayLiteral)
            if value_tokens.lookahead1().peek(syn::token::Bracket) {
                Self::ArrayLiteral { ty, array: {
                    let inner;
                    bracketed!(inner in value_tokens);
                    Punctuated::parse_terminated(&inner)?
                } }
            } else {
                Self::Array { ty, value: value_tokens.parse::<TokenStream>()? }
            }
        } else {
            Self::Single {
                ty: {
                    let ty = input.parse::<Type>()?;
                    if ty.is_void() {
                        return Err(syn::Error::new(ty.span(), "Parameters can't have type Void"))
                    }
                    ty
                },
                value: {
                    let value_tokens;
                    parenthesized!(value_tokens in input);
                    value_tokens.parse::<TokenStream>()?
                },
            }
        })
    }
}
impl ToTokens for Parameter {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Single { ty, value } =>  {
                match ty {
                    Type::Void(_) => panic!("Unreachable"),
                    Type::Byte(ident) | Type::Char(ident)
                    | Type::Bool(ident) | Type::Short(ident)
                    | Type::Int(ident) | Type::Long(ident)
                    | Type::Float(ident) | Type::Double(ident) => {
                        let variant = Ident::new(&first_char_uppercase(ty.to_string()), ident.span());
                        tokens.append_all(quote_spanned! {ident.span()=> ::jni::objects::JValue::#variant });
                        // Parameters of primitive types can be passed in as copy
                        // Bool must be cast to u8
                        tokens.append_all(match ty {
                            Type::Bool(_) => quote_spanned! {value.span()=> (#value as u8) },
                            _ => quote_spanned! {value.span()=> (#value) }
                        })
                    },
                    Type::Object(class) => {
                        tokens.append_all(quote_spanned! {class.span()=> ::jni::objects::JValue::Object });
                        // Parameters of type object need to be passed in as reference
                        tokens.append_all(quote_spanned! {value.span()=> (&(#value)) })
                    },
                }
            },
            Self::Array { ty, value } => {
                tokens.append_all(quote_spanned! {ty.span()=> ::jni::objects::JValue::Object });
                // Parameters of type object need to be passed in as reference
                tokens.append_all(quote_spanned! {value.span()=> (&(#value)) })
            },
            Self::ArrayLiteral { ty, array } => {
                tokens.append_all(quote_spanned! {ty.span()=> ::jni::objects::JValue::Object });
                // Parameters of type object need to be passed in as reference
                let len = LitInt::new(&array.iter().count().to_string(), array.span());
                match ty {
                    Type::Void(_) => panic!("Unreachable"),
                    Type::Byte(ident) | Type::Bool(ident)
                    | Type::Char(ident) | Type::Short(ident)
                    | Type::Int(ident) | Type::Long(ident)
                    | Type::Float(ident) | Type::Double(ident) => {
                        // Bool must be changed to boolean
                        let ident = match ty {
                            Type::Bool(ident) => &Ident::new("boolean", ident.span()),
                            _ => ident
                        };
                        // Values of Boolean array must be cast to u8
                        let values = array.iter();
                        let values = match ty {
                            Type::Bool(_) => quote! { #(#values as u8),* },
                            _ => quote! { #(#values),* }
                        };
                        let new_array_fn = Ident::new(&format!("new_{ident}_array"), array.span());
                        let new_array_err = LitStr::new(&format!("Failed to create Java {ident} array: {{err}}"), array.span());
                        let fill_array_fn = Ident::new(&format!("set_{ident}_array_region"), array.span());
                        let fill_array_err = LitStr::new(&format!("Error filling {ident} array: {{err}}"), array.span());
                        // Create a Java array Object from the array literal
                        tokens.append_all(quote_spanned! {array.span()=> (&{
                            let array = env.#new_array_fn(#len)
                                .inspect_err(|err| println!(#new_array_err)).unwrap();
                            env.#fill_array_fn(&array, 0, &[ #values ])
                                .inspect_err(|err| println!(#fill_array_err)).unwrap();
                            ::jni::objects::JObject::from(array)
                        }) })
                    },
                    Type::Object(class) => {
                        let new_array_err = LitStr::new(&format!("Failed to create Java Object \"{class}\" array: {{err}}"), array.span());
                        let set_val_err = LitStr::new(&format!("Failed to set the value of Object array at index {{i}}: {{err}}"), array.span());
                        let class_path = LitStr::new(&class.to_string(), array.span());
                        // Create the array
                        let mut tt = quote! {
                            let array = env.new_object_array(#len, #class_path, unsafe { ::jni::objects::JObject::from_raw(::std::ptr::null_mut()) } )
                                .inspect_err(|err| println!(#new_array_err)).unwrap();
                        };
                        // Fill the array
                        for (i, element) in array.iter().enumerate() {
                            tt = quote! { #tt
                                env.set_object_array_element(&array, #i, #element)
                                    .inspect_err(|err| println!(#set_val_err)).unwrap();
                            }
                        }
                        // Return the array
                        tt = quote! { #tt ::jni::objects::JObject::from(array) };
                        tokens.append_all(quote_spanned! {array.span()=> (&{ #tt }) })
                    }
                }
            }
        }
    }
}

/// The return type of a JNI method call.
/// 
/// The caller of the macro can assert that the JNI method call will result in one of the following:
/// 1. [`Type`] if the function being called can't return `NULL` or throw an `exception` (e.g. `bool` or `java.lang.String`).
/// 2. `Option<ClassPath>` if the return type is an [`Object`][Type::Object] that could be **NULL**.
///    Java *does not allow* primitive types (i.e. not Object) to be **NULL**, so [Self::Option] can only be used with a [ClassPath].
/// 3. `Result<Type | Option<ClassPath>, String>` if the method call can throw an **Exception**.
/// TODO: might change to `impl FromException`
pub enum Return {
    Assertive(Type),
    Option(ClassPath),
    /// Holds the Ok Type (a Type or Option), and the Err Type (String for now).
    Result(ResultType, ()/*syn::Path*/)
}
impl Return {
    // Helper function that parses the content of the option variant.
    // Helps avoid code repetition.
    fn parse_option(input: syn::parse::ParseStream, fork: syn::parse::ParseStream) -> syn::Result<ClassPath> {
        input.advance_to(&fork);
        // Parse generic arguement
        input.parse::<Token![<]>()
            .map_err(|err| input.error(format!("Option takes generic arguments; {err}")))?;
        let class = match input.parse::<Type>()? {
            Type::Object(path) => path,
            t => return Err(syn::Error::new(t.span(), "Option cannot be used with primitives, only Classes."))
        };
        input.parse::<Token![>]>()
            .map_err(|err| input.error(format!("Option takes only 1 generic argument; {err}")))?;
        Ok(class)
    }
}
impl Parse for Return {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let fork = input.fork();
        // Check if caller uses Option or Result as the return type
        Ok(match fork.parse::<Ident>() {
            Ok(ident) =>
                match ident.to_string().as_str() {
                    "Option" => Self::Option(Self::parse_option(input, &fork)?),
                    "Result" => {
                        input.advance_to(&fork);
                        // Parse generic arguements
                        input.parse::<Token![<]>()
                            .map_err(|err| input.error(format!("Result takes generic arguments; {err}")))?;
                        let ok_type = input.parse()?;
                        input.parse::<Token![,]>()
                            .map_err(|err| input.error(format!("Result takes 2 generic arguments; {err}")))?;
                        let err_type = input.parse::<syn::Path>()?;
                        if !err_type.is_ident("String") {
                            return Err(syn::Error::new(err_type.span(), "For now, only 'String' is suppoerted as the Error type of the Result.\nLater, you could use any Type that implements FromException (doesn't exist yet)."))
                        }
                        input.parse::<Token![>]>()?;
                        Self::Result(ok_type, ()/*err_type*/)
                    },
                    _ => Self::Assertive(input.parse()?),
                },
            // The return type did not start with Ident... weird, but let's continue
            Err(_) => Self::Assertive(input.parse()?),
        })
    }
}

/// The `Ok` Type of a [`Return::Result`] could be a regular type ([`Assertive`][ResultType::Assertive]), or nullable [`Option`]
pub enum ResultType {
    Assertive(Type),
    Option(ClassPath)
}
impl Parse for ResultType {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let fork = input.fork();
        Ok(match fork.parse::<Ident>() {
            Ok(ident) =>
                match ident.to_string().as_str() {
                    "Option" => Self::Option(Return::parse_option(input, &fork)?),
                    "Result" => return Err(syn::Error::new(ident.span(), "Can't nest a Result within a Result")),
                    _ => Self::Assertive(input.parse()?),
                },
            // The return type did not start with Ident... weird, but let's continue
            Err(_) => Self::Assertive(input.parse()?),
        })
    }
}

/// Convert eh first letter of a String into uppercase
fn first_char_uppercase(s: String) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}