
extern crate math_render;
extern crate freetype;

use std::mem;
use math_render::font::*;

fn get_bytes() -> &'static [u8] {
    include_bytes!("testfiles/latinmodern-math.otf")
}

#[test]
fn constants_test() {
    let bytes = get_bytes();
    let library = freetype::Library::init().unwrap();
    let font = MathFont::from_bytes(bytes, 0, &library);

    let latin_moder_consts = [70i32, 50, 1300, 1300, 154, 250, 450, 664, 247, 344, 200, 363, 289,
                              108, 250, 160, 344, 56, 200, 111, 167, 600, 444, 677, 345, 686, 120,
                              280, 111, 600, 200, 167, 394, 677, 345, 686, 40, 120, 40, 40, 120,
                              350, 96, 120, 40, 40, 120, 40, 40, 50, 148, 40, 40, 278, -556, 60];
    for (num, latin_const) in latin_moder_consts.iter().enumerate() {
        let const_index = num as hb::hb_ot_math_constant_t;
        let value = font.get_math_constant(const_index);
        println!("constant num {:?}, named: {:?}; expected value: {:?}, computed value: {:?}",
                 num,
                 const_index,
                 *latin_const,
                 value);
        assert!(value == *latin_const);
    }
}