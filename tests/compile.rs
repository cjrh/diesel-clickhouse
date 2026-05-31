#[test]
fn compile_time_api_contracts() {
    let tests = trybuild::TestCases::new();
    tests.pass("tests/compile/pass/*.rs");
    tests.compile_fail("tests/compile/fail/*.rs");
}
