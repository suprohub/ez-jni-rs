use std::{process::Command, sync::LazyLock};
use jni::{objects::{JObject, JString}, JavaVM};
use ez_jni::call;

static CLASS_DIR: &'static str = "./target/tmp";
static JVM: LazyLock<JavaVM> = LazyLock::new(|| {
    compile_java()
        .unwrap_or_else(|err| panic!("Error compiling Java file: {err}"));
    JavaVM::new(jni::InitArgsBuilder::new()
        .option(format!("-Djava.class.path={CLASS_DIR}"))
        .build()
        .unwrap()
    )
        .unwrap_or_else(|err| panic!("Error starting JavaVM: {err}"))
});

/// Must call it this exact same way: `setup_env!(env)`;
macro_rules! setup_env {
    ($var:ident) => {
        let mut $var = JVM.attach_current_thread_permanently()
            .unwrap_or_else(|err| panic!("Error attaching current thread to JavaVM: {err}"));
    };
}

fn compile_java() -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(CLASS_DIR)?;
    let output = Command::new("javac")
        .args(["./tests/Test.java", "-d", CLASS_DIR])
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into())
    }
    
    Ok(())
}

#[test]
fn return_primitives() {
    setup_env!(env);
    // Java primitive return types
    let _: () = call!(static me.test.Test::getVoid() -> void);
    let _: bool = call!(static me.test.Test::getBoolean() -> boolean);
    let _: char = call!(static me.test.Test::getChar() -> char);
    let _: i8 = call!(static me.test.Test::getByte() -> byte);
    let _: i16 = call!(static me.test.Test::getShort() -> short);
    let _: i32 = call!(static me.test.Test::getInt() -> int);
    let _: i64 = call!(static me.test.Test::getLong() -> long);
    let _: f32 = call!(static me.test.Test::getFloat() -> float);
    let _: f64 = call!(static me.test.Test::getDouble() -> double);
    // Rust primitive return types
    let _: bool = call!(static me.test.Test::getBoolean() -> bool);
    let _: char = call!(static me.test.Test::getChar() -> char);
    let _: u8 = call!(static me.test.Test::getByte() -> u8);
    let _: u16 = call!(static me.test.Test::getShort() -> u16);
    let _: u32 = call!(static me.test.Test::getInt() -> u32);
    let _: u64 = call!(static me.test.Test::getLong() -> u64);
    let _: i8 = call!(static me.test.Test::getByte() -> i8);
    let _: i16 = call!(static me.test.Test::getShort() -> i16);
    let _: i32 = call!(static me.test.Test::getInt() -> i32);
    let _: i64 = call!(static me.test.Test::getLong() -> i64);
    let _: f32 = call!(static me.test.Test::getFloat() -> f32);
    let _: f64 = call!(static me.test.Test::getDouble() -> f64);
}

#[test]
fn return_other() {
    setup_env!(env);
    // Object
    let _: JObject = call!(static me.test.Test::getObject() -> java.lang.Object);
    let _: JString = call!(static me.test.Test::getString() -> java.lang.String);
    // Result
    let r: Result<bool, String> = call!(static me.test.Test::getBoolean() -> Result<bool, String>);
    r.unwrap();
    let r: Result<bool, String> = call!(static me.test.Test::throw_prim() -> Result<bool, String>);
    r.unwrap_err();
    let r: Result<JObject, String> = call!(static me.test.Test::getObject() -> Result<java.lang.Object, String>);
    r.unwrap();
    let r: Result<JObject, String> = call!(static me.test.Test::throw_obj() -> Result<java.lang.Object, String>);
    r.unwrap_err();
    // Option
    let r: Option<JObject> = call!(static me.test.Test::getObject() -> Option<java.lang.Object>);
    r.unwrap();
    let r: Option<JObject> = call!(static me.test.Test::nullable() -> Option<java.lang.Object>);
    assert!(r.is_none());
    // Result<Option<_>, _>
    let _: Result<Option<JObject>, String> = call!(static me.test.Test::getObject() -> Result<Option<java.lang.Object>, String>);
}

#[test]
fn arguments() {
    setup_env!(env);
    // Arguments
    call!(static me.test.Test::multiArg(boolean(true), char('a'), byte(1i8), short(1i16), int(1i32), long(1i64), float(1f32), double(1f64), java.lang.Object(JObject::null())) -> void);
    call!(static me.test.Test::multiArg(bool(true), char('a'), u8(1u8), u16(1u16), u32(1u32), u64(1u64), f32(1f32), f64(1f64), java.lang.Object(JObject::null())) -> void);
    call!(static me.test.Test::arrayArg(
        [boolean](env.new_boolean_array(0).unwrap()),
        [char](env.new_char_array(0).unwrap()),
        [byte](env.new_byte_array(0).unwrap()),
        [short](env.new_short_array(0).unwrap()),
        [int](env.new_int_array(0).unwrap()),
        [long](env.new_long_array(0).unwrap()),
        [float](env.new_float_array(0).unwrap()),
        [double](env.new_int_array(0).unwrap()),
        [java.lang.Object](JObject::null())
    ) -> void);
    call!(static me.test.Test::arrayArg(
        [boolean]([true, false]),
        [char](['a', 'b']),
        [byte]([1i8, 2]),
        [short]([1i16, 2]),
        [int]([1i32, 2]),
        [long]([1i64, 2]),
        [float]([1f32, 2.0]),
        [double]([1f64, 2.0]),
        [java.lang.Object]([])
    ) -> void);
    call!(static me.test.Test::arrayArg(
        [bool]([true, false]),
        [char](['a', 'b']),
        [u8]([1u8, 2]),
        [u16]([1u16, 2]),
        [u32]([1u32, 2]),
        [u64]([1u64, 2]),
        [f32]([1f32, 2.0]),
        [f64]([1f64, 2.0]),
        [java.lang.Object]([])
    ) -> void);
}