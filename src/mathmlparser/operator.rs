use std::str::FromStr;
use std::ops::Not;

use types::{MathItem, Length, MathExpression};

use super::MExpression;
use super::operator_dict;

bitflags! {
    pub flags Flags: u8 {
        const SYMMETRIC         = 0b00000001,
        const FENCE             = 0b00000010,
        const STRETCHY          = 0b00000100,
        const SEPARATOR         = 0b00001000,
        const ACCENT            = 0b00010000,
        const LARGEOP           = 0b00100000,
        const MOVABLE_LIMITS    = 0b01000000,
    }
}

impl Default for Flags {
    fn default() -> Flags {
        Flags::empty()
    }
}

pub struct FormParsingError {
    pub unknown_str: String
}

#[derive(PartialEq, Eq, Copy, Clone, Debug, Ord, PartialOrd)]
pub enum Form {
    Prefix,
    Infix,
    Postfix,
}

impl Default for Form {
    fn default() -> Form {
        Form::Infix
    }
}

impl FromStr for Form {
    type Err = FormParsingError;
    fn from_str(s: &str) -> Result<Form, FormParsingError> {
        match s {
            "prefix" => Ok(Form::Prefix),
            "infix" => Ok(Form::Infix),
            "postfix" => Ok(Form::Postfix),
            _ => Err(FormParsingError { unknown_str: s.to_string() }),
        }
    }
}

#[derive(Debug, Copy, Clone, Default)]
pub struct Attributes {
    pub character: Option<char>,
    pub form: Option<Form>,
    pub lspace: Option<Length>,
    pub rspace: Option<Length>,
    pub flags: Flags,
    pub user_overrides: Flags,
}

impl Attributes {
    pub fn set_user_override(&mut self, flag: Flags, is_true: bool) {
        self.user_overrides.insert(flag);
        if is_true {
            self.flags.insert(flag);
        } else {
            self.flags.remove(flag);
        }
    }
}

pub fn insert_space_for_operator(list: &mut Vec<MExpression>, mut index: usize) {
    let operator_attrs = list[index].user_info.operator_attrs.unwrap();
    let lspace = operator_attrs.lspace.expect("operator has no lspace");
    let rspace = operator_attrs.rspace.expect("operator has no rspace");
    if !lspace.is_null() {
        let left_space = MathItem::Space {
            width: lspace,
            ascent: Length::Em(0f32),
            descent: Length::Em(0f32),
        };
        let left_space = MathExpression { content: left_space, ..Default::default() };
        list.insert(index, left_space);
        index += 1;
    }
    if !rspace.is_null() {
        let right_space = MathItem::Space {
            width: rspace,
            ascent: Length::Em(0f32),
            descent: Length::Em(0f32),
        };
        let right_space = MathExpression { content: right_space, ..Default::default() };
        list.insert(index + 1, right_space);
    }
}

// (Embellished) operators are treated specially because their default attribute values depend
// on the surrounding elements.
pub fn process_operators(list: &mut Vec<MExpression>) {
    let mut first_non_ws_index = -1;
    let mut last_non_ws_index = 0;
    let operator_indices = list.iter_mut()
        .enumerate()
        .filter(|&(_, ref elem)| elem.is_space().not())
        .inspect(|&(index, _)| {
            if first_non_ws_index == -1 {
                first_non_ws_index = index as isize;
            }
            last_non_ws_index = index;
        })
        .filter(|&(_, ref elem)| elem.user_info.is_operator())
        .by_ref()
        .map(|(index, _)| {
            index
        })
        .collect::<Vec<_>>();

    for index in &operator_indices {
        let elem = &mut list[*index];
        elem.user_info.operator_attrs = elem.user_info.operator_attrs.or(Some(Default::default()));
        if first_non_ws_index as usize != last_non_ws_index {
            if *index == first_non_ws_index as usize {
                set_default_form(elem, Form::Prefix);
            } else if *index == last_non_ws_index {
                set_default_form(elem, Form::Postfix);
            }
        }
        set_default_form(elem, Form::Infix);
        guess_operator_space(elem);
    }

    let mut iterator = operator_indices.iter();
    while let Some(index) = iterator.next_back() {
        insert_space_for_operator(list, *index);
    }
}

fn set_default_form(elem: &mut MExpression, form: Form) {
    elem.user_info
        .operator_attrs
        .as_mut()
        .map(|op_attrs| op_attrs.form = op_attrs.form.or(Some(form)));
}

fn guess_operator_space(elem: &mut MExpression) {
    let operator_attrs = elem.user_info.operator_attrs.as_mut().unwrap();
    if operator_attrs.lspace.and(operator_attrs.rspace).is_some() {
        return;
    }

    let form = operator_attrs.form.expect("operator has no form");
    let entry = operator_attrs.character
        .and_then(|chr| operator_dict::find_entry(chr, form))
        .unwrap_or_default();

    if operator_attrs.lspace.is_none() {
        operator_attrs.lspace = Some(Length::Em(entry.lspace as f32 / 18.0f32));
    }
    if operator_attrs.rspace.is_none() {
        operator_attrs.rspace = Some(Length::Em(entry.rspace as f32 / 18.0f32));
    }

    operator_attrs.flags = (operator_attrs.user_overrides & operator_attrs.flags) |
                           (!operator_attrs.user_overrides & entry.flags);
}