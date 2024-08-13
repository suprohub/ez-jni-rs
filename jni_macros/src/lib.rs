use std::sync::RwLock;
use call::{MethodCall, ObjectMethod, Return, StaticMethod, Type};
use either::Either;
use proc_macro2::Span;
use quote::{quote, quote_spanned, ToTokens};
use syn::{spanned::Spanned, GenericParam, Ident, ItemFn, LitStr};
use proc_macro::TokenStream;

mod call;

static PACKAGE_NAME: RwLock<Option<String>> = RwLock::new(None);

/// Defines the name of the package name to use in the names of exported JNI functions.
/// 
/// Must be all lowercase and have no hyphens.
/// 
/// Example: `package!("me.author.packagename")`
#[proc_macro]
pub fn package(input: TokenStream) -> TokenStream {
    let package_name = syn::parse_macro_input!(input as LitStr).value();
    *PACKAGE_NAME.write().unwrap() = Some(package_name);
    return TokenStream::new()
}

/// Changes a function's signature so that it can be called from external Java code.
/// Requires that the function be defined with `pub` visibility, no generic types, no lifetime named "local", and no arguments named "env" or "_class".
/// 
/// Also takes the name of a Java Class or extra package data where this function is defined in the Java side.
/// 
/// ### Example
/// ```
/// # use jni_macros::{package, jni_fn};
/// package!("me.author.packagename")
/// 
/// #[jni_fn("MyClass")]
/// pub fn hello_world(s: JString) {
///     // body
/// }
/// ```
/// expands to
/// 
/// ```
/// #[no_mangle]
/// pub extern "system" fn Java_me_author_packagename_myClass_hello_1world<'local>(
///     mut env: ::jni::JNIEnv<'local>, _class: ::jni::objects::JClass<'local>,
///     s: JString
/// ) {
///     // body
/// }
/// ```
#[proc_macro_attribute]
pub fn jni_fn(attr_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut input = syn::parse_macro_input!(input as ItemFn);
    let extra_package_data = if attr_args.is_empty() { None } else {
        Some(syn::parse_macro_input!(attr_args as LitStr).value())
    };

    // Function must have 'pub' visibility
    match input.vis {
        syn::Visibility::Public(_) => {},
        _ => return error_spanned(input.sig.span(), "Function must have 'pub' visibility").into()
    }

    // Function can't have a lifetime named "local"
    if let Some(lifetime) = input.sig.generics.params.iter()
        .filter_map(|g| match g {
            GenericParam::Lifetime(lifetime) => Some(lifetime), _ => None
        })
        .find(|&lifetime| lifetime.lifetime.ident.to_string() == "local")
    {
        return error_spanned(lifetime.lifetime.span(), "Function can't have a lifetime named \"local\"").into()
    }

    // Function can't have generic types
    let generic_types = input.sig.generics.params.iter()
        .filter(|g| match g {
            GenericParam::Type(_) | GenericParam::Const(_) => true,
            _ => false
        })
        .map(|g| error_spanned(g.span(), "Function can't have generic types"))
        .fold(quote!{}, |acc, next| quote! { #acc #next }.into());
    if !generic_types.is_empty() {
        return generic_types.into()
    }

    // Function can't have arguments named "env" or "_class"
    let bad_arguments = input.sig.inputs.iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(arg) => Some(arg),
            _ => None
        })
        .filter(|&arg| ["env", "_class"].contains(&arg.pat.to_token_stream().to_string().as_str()))
        .map(|arg| error_spanned(arg.span(), format!("Function can't have an argument named \"{}\"", arg.pat.to_token_stream())))
        .fold(quote!{}, |acc, next| quote! { #acc #next }.into());
    if !bad_arguments.is_empty() {
        return bad_arguments.into()
    }

    // Change name of function
    let class_path = match &*PACKAGE_NAME.read().unwrap() {
        // Process package data to use underscores (_)
        Some(package_name) =>
            format!("{package_name}.{}", extra_package_data.unwrap_or("".to_string()))
                .replace(['.', '/'], "_"),
        None => return error("Macro jni_macros::package! has not been called.").into()
    };
    let name = input.sig.ident.to_string().replace('_', "_1");
    input.sig.ident = Ident::new(&format!("Java_{class_path}_{name}"), input.sig.ident.span());
    // Convert to system ABI
    input.attrs.push(syn::parse_quote!(#[no_mangle]));
    input.sig.abi = Some(syn::parse_quote!(extern "system"));
    // Add 'local lifetime
    input.sig.generics.params.push(GenericParam::Lifetime(syn::parse_quote!('local)));
    // Add env and _class arguments
    input.sig.inputs.insert(0, syn::FnArg::Typed(syn::parse_quote!(mut env: ::jni::JNIEnv<'local>)));
    input.sig.inputs.insert(1, syn::FnArg::Typed(syn::parse_quote!(_class: ::jni::objects::JClass<'local>)));

    quote! { #input }.into()
}

/// A macro that helps make JNI Method calls less verbose and easier to use in Rust.
/// 
/// Can be used to call **`static methods`** on Java classes:
/// ```text
/// call!(static me.author.ClassName::methodName(int(arg1), java.lang.String(arg2)) -> int)
///                 Primitive type parameter --->\_______/  \____________________/     \_/
///                   Object type parameter --------------------------^                 |
///                      Return type        --------------------------------------------^
/// ```
/// Or to call **object methods**:
/// ```no_run
/// call!(object.methodName() -> void)
/// ```
/// 
/// # Syntax
/// 
/// To use the **static method** call, prepend the call with `static`, then the path to the *class name*,
/// and finally a *PathSeparator* (`::`) to separate the class from the method name.
/// ```nor_run
/// call!(static me.author.ClassName::methodName() -> void)
/// ```
/// 
/// To use an **object method** call, simply put a *variable name* that is of type `JObject` (or put an *expression* that resolves to a `JObject` in parentheses).
/// For example, the *object* in the following line could be `my_object`, or `(something.object())`:
/// ```no_run
/// call!(object.myMethod() -> void)
/// ```
/// 
/// ## Parameters
/// 
/// The parameters of the method call are placed inside perentheses after the method name,
/// and can be *primitive values*, *object values*, or *arrays of either* (type wrapped in brackets).
/// 
/// All parameters have a **type**, and a **value** (wrapped in parenthesis).
/// The value goes in parenthesis after the parameter type, and is any expression that resolves to the right type.
/// For *arrays*, the value could be one of the `JPrimitiveArray`s or `JObjectArray`, or an array literal of either.
/// 
/// ```no_run
/// int(2 + 2) // primitive
/// me.author.ClassName(value) // object
/// [bool]([true, false]) // primitive array
/// [java.lang.String](value) // object array
/// ```
/// 
/// ## Return
/// 
/// The parameters are followed by a *return arrow* `->` and the *return type*.
/// The return type may be concrete *primitive or Class*, an [`Option`] of a nullable Class, or a [`Result`] of a *primitive or Class*.
/// 
/// - Use the **concrete type** when the Java method being called can't return *`NULL`* or throw an *exception*,
///   such as when it is marked with `@NonNull`.
/// - Use **`Option`** when the method can return a *`NULL`* value.
/// - Use **`Result`** when the method can throw an *exception*, e.g. `void method() throws Exception { ... }`
///
/// Here are some examples of return types:
/// ```no_run
/// -> int OR java.lang.String
/// -> Option<java.lang.String>
/// -> Result<int, String> OR Result<java.lang.String, String>
/// ```
/// Note that `Option` can't be used with *primitive types* because those can't be `NULL` in Java.
/// 
/// For now, the `Err` of the [`Result`] can only be of type String,
/// but this will change in the future to allow any type that implements `FromThrowable` (a trait that doesn't yet exist).
#[proc_macro]
pub fn call(input: TokenStream) -> TokenStream {
    let call = syn::parse_macro_input!(input as MethodCall);
    
    let name = call.method_name.to_string();
    let signature = {
        let mut buf = String::from("(");
        for param in &call.parameters {
            // Array types in signature have an opening bracket prepended to the type
            if param.is_array() {
                buf.push('[');
            }
            buf.push_str(&param.ty().sig_type());
        }
        buf.push(')');
        buf.push_str(&match &call.return_type {
            Return::Assertive(ty) | Return::Result(ty, _) => ty.sig_type(),
            Return::Option(class) => class.sig_type()
        });
        LitStr::new(&buf, call.parameters.span())
    };
    let parameters = {
        let params = call.parameters.iter();
        quote!{ &[ #(#params),* ] }
    };
    // Extra function calls, such as .l() to make the result into a JObject.
    let extras = {
        let mut tt = quote!{};
        
        // Induce panic when fails to call method
        let call_failed_msg = match &call.call_type {
            Either::Left(StaticMethod(path)) => format!("Failed to call static method {name}() on {path}: {{err}}"),
            Either::Right(ObjectMethod(_)) => format!("Failed to call {name}(): {{err}}")
        };
        tt = quote!{ #tt .inspect_err(|err| panic!(#call_failed_msg)).unwrap() };
        // Induce panic when the returned value is not the expected type
        let incorrect_type_msg = match &call.return_type {
            Return::Assertive(Type::Object(class))
            | Return::Result(Type::Object(class), _)
            | Return::Option(class) => format!("Expected {name}() to return {class}: {{err}}"),
            Return::Assertive(ty)
            | Return::Result(ty, _) => format!("Expected {name}() to return {ty}: {{err}}")
        };
        let sig_char = match &call.return_type {
            Return::Assertive(ty) | Return::Result(ty, _) => Ident::new(ty.sig_char().to_string().as_str(), ty.span()),
            Return::Option(class) => Ident::new("l", class.span()),
        };
        tt = quote!{ #tt .#sig_char().inspect_err(|err| panic!(#incorrect_type_msg)).unwrap() };
        tt
    };
    
    // Build the macro function call
    let jni_call = match call.call_type {
        Either::Left(StaticMethod(class)) => {
            let class = LitStr::new(&class.to_string(), class.span());
            quote! {
                env.call_static_method(#class, #name, #signature, #parameters)
                    #extras
            }
        },
        Either::Right(ObjectMethod(object)) => quote! {
            env.call_method(&(#object), #name, #signature, #parameters)
                #extras
        },
    };
    
    let non_null_msg = format!("Expected Object returned by {name}() to not be NULL");
    let null_check = quote! { if __call_result.is_null() { panic!(#non_null_msg) } };
    match call.return_type {
        // Move the result of the method call to an Option if the caller expects that the returned Object could be NULL.
        Return::Option(_) => quote!{ {
            // TODO: type object??
            let __call_result = #jni_call;
            if __call_result.is_null() {
                None
            } else {
                Some(__call_result)
            }
        } },
        Return::Assertive(ty) => {
            match ty {
                Type::Object(_) => quote!{ {
                    let __call_result = #jni_call;
                    #null_check;
                    __call_result
                } },
                _ => jni_call
            }
        },
        // Move the result of the method call to a Result if the caller expects that the method could throw.
        Return::Result(ty, _) => {
            let null_check = match ty {
                Type::Object(_) => null_check,
                _ => quote!{ }
            };
            quote!{ {
                let __call_result = #jni_call;
                #null_check;
                crate::utils::catch_exception(env).map(|_| __call_result)
            } }
        },
    }.into()
}

fn error(err: impl AsRef<str>) -> proc_macro2::TokenStream {
    let err = err.as_ref();
    quote! { compile_error!(#err); }
}
fn error_spanned(span: Span, err: impl AsRef<str>) -> proc_macro2::TokenStream {
    let err = err.as_ref();
    quote_spanned! {span=>
        compile_error!(#err);
    }
}
