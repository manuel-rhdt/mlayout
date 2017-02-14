#![allow(unused_variables, dead_code)]
use types::*;
use std::iter::IntoIterator;
use std::cmp::{max, min};
use std::fmt::Debug;

use super::shaper::{MathShaper, MathConstant, ShapedGlyph};
use super::math_box::{MathBox, Extents, Point};
use super::multiscripts::*;
use super::stretchy::*;

#[derive(Copy, Clone)]
pub struct LayoutOptions<'a> {
    pub shaper: &'a MathShaper,
    pub style: LayoutStyle,
    pub stretch_size: Option<Extents<i32>>,
    pub as_accent: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct StretchProperties {
    pub intrinsic_size: u32,
    pub horizontal: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct OperatorProperties {
    pub stretch_properties: Option<StretchProperties>,
    pub leading_space: i32,
    pub trailing_space: i32,
}

impl Length {
    fn to_font_units(self, shaper: &MathShaper) -> i32 {
        if self.is_null() {
            return 0;
        }
        match self.unit {
            LengthUnit::Em => (shaper.em_size() as f32 * self.value) as i32,
            LengthUnit::Point => {
                Length::em(self.value / shaper.ppem().0 as f32).to_font_units(shaper)
            }
            LengthUnit::DisplayOperatorMinHeight => {
                (shaper.math_constant(MathConstant::DisplayOperatorMinHeight) as f32 *
                 self.value) as i32
            }
        }
    }
}


fn clamp<T: Ord, U: Into<Option<T>>>(value: T, min: U, max: U) -> T {
    if let Some(min) = min.into() {
        if value < min {
            return min;
        };
    }
    if let Some(max) = max.into() {
        if value > max {
            return max;
        };
    }
    value
}

fn math_box_from_shaped_glyphs<'a, T: 'a, I>(glyphs: I,
                                             options: LayoutOptions<'a>)
                                             -> MathBox<'a, T>
    where I: 'a + IntoIterator<Item = ShapedGlyph>
{
    let mut cursor = Point { x: 0, y: 0 };
    let scale = options.shaper.scale_factor_for_script_level(options.style.script_level);
    let scale = PercentScale2D {
        horiz: scale,
        vert: scale,
    };
    let iterator = glyphs.into_iter().map(move |ShapedGlyph { mut origin, mut advance, glyph }| {
        let glyph = Glyph {
            glyph_code: glyph,
            scale: scale,
        };
        origin = origin * scale;
        advance = advance * scale;
        let mut math_box = MathBox::with_glyph(glyph, options.shaper);
        origin.y = -origin.y;
        math_box.origin = origin + cursor;
        cursor.x += advance.x;
        cursor.y += advance.y;
        math_box
    });
    MathBox::with_iter(Box::new(iterator))
}

/// The trait that every Item in a math list satisfies so that the entire math list can be
/// laid out.
pub trait MathBoxLayout<'a, T> {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T>;
    fn operator_properties(&self, options: LayoutOptions<'a>) -> Option<OperatorProperties> {
        None
    }
    fn can_stretch(&self, options: LayoutOptions<'a>) -> bool {
        self.operator_properties(options)
            .map(|operator_properties| operator_properties.stretch_properties.is_some())
            .unwrap_or_default()
    }
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for Field {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        match self {
            Field::Empty => MathBox::default(),
            Field::Glyph(glyph) => MathBox::with_glyph(glyph, options.shaper),
            Field::Unicode(content) => {
                let shaper = options.shaper;
                math_box_from_shaped_glyphs(shaper.shape_string(&content, options.style), options)
            }
        }
    }
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for Vec<MathExpression<T>> {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        let boxes = layout_strechy_list(self, options);

        let mut cursor = 0i32;
        let mut previout_italic_correction = 0;
        let layouted = boxes.map(move |mut math_box| {
            // apply italic correction if current glyph is upright
            if math_box.italic_correction() == 0 {
                cursor += previout_italic_correction;
            }
            math_box.origin.x += cursor;
            cursor += math_box.advance_width();
            previout_italic_correction = math_box.italic_correction();
            math_box
        });
        MathBox::with_iter(Box::new(layouted))
    }
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for Atom<T> {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        layout_sub_superscript(self.bottom_right, self.top_right, self.nucleus, options)
    }

    fn operator_properties(&self, options: LayoutOptions<'a>) -> Option<OperatorProperties> {
        self.nucleus.operator_properties(options)
    }
}

fn layout_sub_superscript<'a, T: 'a + Debug>(subscript: MathExpression<T>,
                                             superscript: MathExpression<T>,
                                             nucleus: MathExpression<T>,
                                             options: LayoutOptions<'a>)
                                             -> MathBox<'a, T> {
    let mut subscript_options = options;
    subscript_options.style = options.style.subscript_style();
    let mut superscript_options = options;
    superscript_options.style = options.style.superscript_style();
    let subscript = subscript.into_option().map(|x| x.layout(subscript_options));
    let superscript = superscript.into_option().map(|x| x.layout(superscript_options));
    let nucleus_is_largeop = match nucleus.content {
        MathItem::Operator(Operator { is_large_op, .. }) => is_large_op,
        _ => false,
    };
    let mut nucleus = nucleus.layout(options);

    let space_after_script = options.shaper.math_constant(MathConstant::SpaceAfterScript);

    if subscript.is_none() && superscript.is_none() {
        return nucleus;
    }

    let mut result = Vec::with_capacity(4);
    match (subscript, superscript) {
        (Some(mut subscript), Some(mut superscript)) => {
            let (sub_shift, super_shift) = get_subsup_shifts(&subscript,
                                                             &superscript,
                                                             &nucleus,
                                                             options.shaper,
                                                             options.style);
            position_attachment(&mut subscript,
                                &mut nucleus,
                                nucleus_is_largeop,
                                CornerPosition::BottomRight,
                                sub_shift,
                                options.shaper);
            position_attachment(&mut superscript,
                                &mut nucleus,
                                nucleus_is_largeop,
                                CornerPosition::TopRight,
                                super_shift,
                                options.shaper);
            result.push(nucleus);
            result.push(subscript);
            result.push(superscript);
        }
        (Some(mut subscript), None) => {
            let sub_shift = get_subscript_shift_dn(&subscript, &nucleus, options.shaper);
            position_attachment(&mut subscript,
                                &mut nucleus,
                                nucleus_is_largeop,
                                CornerPosition::BottomRight,
                                sub_shift,
                                options.shaper);
            result.push(nucleus);
            result.push(subscript);
        }
        (None, Some(mut superscript)) => {
            let super_shift =
                get_superscript_shift_up(&superscript, &nucleus, options.shaper, options.style);
            position_attachment(&mut superscript,
                                &mut nucleus,
                                nucleus_is_largeop,
                                CornerPosition::TopRight,
                                super_shift,
                                options.shaper);
            result.push(nucleus);
            result.push(superscript);
        }
        (None, None) => unreachable!(),
    }

    let mut space = MathBox::empty(Extents::new(0, space_after_script, 0, 0));
    space.origin.x = result.iter()
        .map(|math_box| math_box.origin.x + math_box.advance_width())
        .max()
        .unwrap_or_default();
    result.push(space);

    MathBox::with_vec(result)
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for OverUnder<T> {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        if self.is_limits && options.style.math_style == MathStyle::Inline {
            return layout_sub_superscript(self.under, self.over, self.nucleus, options);
        }
        let nucleus_is_largeop = match self.nucleus.content {
            MathItem::Operator(Operator { is_large_op, .. }) => is_large_op,
            _ => false,
        };
        let nucleus_is_horizontally_stretchy = self.nucleus.can_stretch(options);

        let mut over_options = LayoutOptions {
            style: options.style.inline_style(),
            stretch_size: None,
            ..options
        };
        if !self.over_is_accent {
            over_options.style = over_options.style.superscript_style();
        }
        let mut under_options = LayoutOptions {
            style: options.style.inline_style(),
            stretch_size: None,
            ..options
        };
        if !self.under_is_accent {
            under_options.style = under_options.style.subscript_style();
        }
        let mut expressions = [(self.nucleus.into_option(), options, false),
                               (self.over.into_option(), over_options, self.over_is_accent),
                               (self.under.into_option(), under_options, self.under_is_accent)];
        let mut boxes = [None, None, None];

        for (index, &mut (ref mut expr, options, ..)) in expressions.iter_mut().enumerate() {
            // first take and layout non-stretchy subexpressions
            if !expr.as_ref().map(|x| x.can_stretch(options)).unwrap_or(false) {
                boxes[index] = expr.take().map(|expr| expr.layout(options));
            }
        }
        // get the maximal width of the non-stretchy items
        let mut max_width = boxes.iter()
            .map(|math_box| math_box.as_ref().map(|x| x.extents().width).unwrap_or_default())
            .max()
            .unwrap_or_default();

        // the OverUnder has to stretch to at least the current stretch size
        if let Some(Extents { width: stretch_width, .. }) = options.stretch_size {
            max_width = max(max_width, stretch_width);
        }

        // layout the rest
        for (index, &mut (ref mut expr, mut options, as_accent)) in expressions.iter_mut().enumerate() {
            options.stretch_size = options.stretch_size.or(Some(Default::default()))
                .map(|size| Extents { width: max_width, ..size });
            options.as_accent = as_accent;
            if let Some(stretched_box) = expr.take().map(|expr| expr.layout(options)) {
                boxes[index] = Some(stretched_box);
            }
        }

        let nucleus = boxes[0].take().unwrap_or_default();
        let nucleus = if let Some(over) = boxes[1].take() {
            let (_, LayoutOptions { style, shaper, .. }, ..) = expressions[1];
            layout_over_or_under(over,
                                 nucleus,
                                 shaper,
                                 style,
                                 true,
                                 self.over_is_accent,
                                 nucleus_is_largeop,
                                 nucleus_is_horizontally_stretchy)
        } else {
            nucleus
        };

        if let Some(under) = boxes[2].take() {
            let (_, LayoutOptions { style, shaper, .. }, ..) = expressions[2];
            layout_over_or_under(under,
                                 nucleus,
                                 shaper,
                                 style,
                                 false,
                                 self.under_is_accent,
                                 nucleus_is_largeop,
                                 nucleus_is_horizontally_stretchy)
        } else {
            nucleus
        }
    }

    fn operator_properties(&self, options: LayoutOptions<'a>) -> Option<OperatorProperties> {
        self.nucleus.operator_properties(options)
    }
}

fn layout_over_or_under<'a, T: 'a>(mut attachment: MathBox<'a, T>,
                                   mut nucleus: MathBox<'a, T>,
                                   shaper: &MathShaper,
                                   style: LayoutStyle,
                                   as_over: bool,
                                   as_accent: bool,
                                   nucleus_is_large_op: bool,
                                   nucleus_is_horizontally_stretchy: bool)
                                   -> MathBox<'a, T> {
    let mut gap = 0;
    let mut shift = 0;
    if nucleus_is_large_op {
        if as_over {
            gap = shaper.math_constant(MathConstant::UpperLimitGapMin);
            shift = shaper.math_constant(MathConstant::UpperLimitBaselineRiseMin) +
                    nucleus.extents().ascent;
        } else {
            gap = shaper.math_constant(MathConstant::LowerLimitGapMin);
            shift = shaper.math_constant(MathConstant::LowerLimitBaselineDropMin) +
                    nucleus.extents().descent;
        }
    } else if nucleus_is_horizontally_stretchy {
        if as_over {
            gap = shaper.math_constant(MathConstant::StretchStackGapBelowMin);
            shift = shaper.math_constant(MathConstant::StretchStackTopShiftUp);
        } else {
            gap = shaper.math_constant(MathConstant::StretchStackGapAboveMin);
            shift = shaper.math_constant(MathConstant::StretchStackBottomShiftDown);
        }
    } else if !as_accent {
        gap = if as_over {
            shaper.math_constant(MathConstant::OverbarVerticalGap)
        } else {
            shaper.math_constant(MathConstant::UnderbarVerticalGap)
        };
        shift = gap;
    }

    let baseline_offset = if as_accent {
        if as_over {
            let accent_base_height = shaper.math_constant(MathConstant::AccentBaseHeight);
            -max(nucleus.extents().ascent - accent_base_height, 0)
        } else {
            nucleus.extents().descent
        }
    } else {
        if as_over {
            -max(shift,
                 nucleus.extents().ascent + gap + attachment.extents().descent)
        } else {
            max(shift,
                nucleus.extents().descent + gap + attachment.extents().ascent)
        }
    };


    attachment.origin.y += nucleus.origin.y;
    attachment.origin.y += baseline_offset;

    // centering
    let center_difference = if as_accent && as_over {
        (nucleus.origin.x + nucleus.top_accent_attachment()) -
        (attachment.origin.x + attachment.top_accent_attachment())
    } else {
        (nucleus.origin.x + nucleus.extents().center()) -
        (attachment.origin.x + attachment.extents().center())
    };
    if center_difference < 0 {
        nucleus.origin.x -= center_difference;
    } else {
        attachment.origin.x += center_difference;
    }

    // LargeOp italic correction
    if nucleus_is_large_op {
        if as_over {
            attachment.origin.x += nucleus.italic_correction() / 2;
        } else {
            attachment.origin.x -= nucleus.italic_correction() / 2;
        }
    }

    // first the attachment then the nucleus to preserve the italic collection of the latter
    MathBox::with_vec(vec![attachment, nucleus])
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for GeneralizedFraction<T> {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        let mut numerator_options = options;
        if options.style.math_style == MathStyle::Display {
            numerator_options.style.math_style = MathStyle::Inline;
        } else {
            numerator_options.style.script_level += 1;
        }
        let denominator_options =
            LayoutOptions { style: numerator_options.style.cramped_style(), ..options };
        let mut numerator = self.numerator.layout(numerator_options);
        let mut denominator = self.denominator.layout(denominator_options);
        let shaper = &options.shaper;

        let axis_height = shaper.math_constant(MathConstant::AxisHeight);
        let default_thickness = shaper.math_constant(MathConstant::FractionRuleThickness);

        let (numerator_shift_up, denominator_shift_dn) = if options.style.math_style ==
                                                            MathStyle::Inline {
            (shaper.math_constant(MathConstant::FractionNumeratorShiftUp),
             shaper.math_constant(MathConstant::FractionDenominatorShiftDown))
        } else {
            (shaper.math_constant(MathConstant::FractionNumeratorDisplayStyleShiftUp),
             shaper.math_constant(MathConstant::FractionDenominatorDisplayStyleShiftDown))
        };

        let (numerator_gap_min, denominator_gap_min) = if options.style.math_style ==
                                                          MathStyle::Inline {
            (shaper.math_constant(MathConstant::FractionNumeratorGapMin),
             shaper.math_constant(MathConstant::FractionDenominatorGapMin))
        } else {
            (shaper.math_constant(MathConstant::FractionNumDisplayStyleGapMin),
             shaper.math_constant(MathConstant::FractionDenomDisplayStyleGapMin))
        };

        let numerator_shift_up = max(numerator_shift_up - axis_height,
                                     numerator_gap_min + default_thickness / 2 +
                                     numerator.extents().descent);
        let denominator_shift_dn = max(denominator_shift_dn + axis_height,
                                       denominator_gap_min + default_thickness / 2 +
                                       denominator.extents().ascent);

        numerator.origin.y -= axis_height;
        denominator.origin.y -= axis_height;

        numerator.origin.y -= numerator_shift_up;
        denominator.origin.y += denominator_shift_dn;

        // centering
        let center_difference = (numerator.origin.x + numerator.extents().center()) -
                                (denominator.origin.x + denominator.extents().center());
        if center_difference < 0 {
            numerator.origin.x -= center_difference;
        } else {
            denominator.origin.x += center_difference;
        }

        // the fraction rule
        let origin = Point {
            x: min(numerator.origin.x + numerator.extents().left_side_bearing,
                   denominator.origin.x + denominator.extents().left_side_bearing),
            y: -axis_height,
        };
        let target = Point {
            x: max(numerator.origin.x + numerator.extents().right_edge(),
                   denominator.origin.x + denominator.extents().right_edge()),
            ..origin
        };
        let fraction_rule = MathBox::with_line(origin, target, default_thickness as u32);

        MathBox::with_vec(vec![numerator, fraction_rule, denominator])
    }

    fn operator_properties(&self, options: LayoutOptions<'a>) -> Option<OperatorProperties> {
        self.numerator.operator_properties(options)
    }
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for Root<T> {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        let shaper = options.shaper;
        let line_thickness = shaper.math_constant(MathConstant::RadicalRuleThickness);
        let vertical_gap = if options.style.math_style == MathStyle::Inline {
            shaper.math_constant(MathConstant::RadicalVerticalGap)
        } else {
            shaper.math_constant(MathConstant::RadicalDisplayStyleVerticalGap)
        };
        let extra_ascender = shaper.math_constant(MathConstant::RadicalExtraAscender);

        // calculate the needed surd height based on the height of the radicand
        let mut radicand = self.radicand.layout(options);
        let needed_surd_height = (radicand.extents().height() + vertical_gap + line_thickness) as
                                 u32;

        // draw a stretched version of the surd
        let mut surd = options.shaper.shape_string("√", options.style);
        let surd = match surd.next() {
            Some(shaped_glyph) => {
                options.shaper
                    .stretch_glyph(shaped_glyph.glyph, false, false, needed_surd_height)
                    .expect("could not stretch surd")
            }
            None => Box::new(::std::iter::empty()),
        };
        let mut surd = math_box_from_shaped_glyphs(surd, options);

        // raise the surd so that its ascent is at least the radicand's ascender plus the radical
        // gap plus the line thickness of the radical rule
        let surd_excess_height = surd.extents().height() -
                                 (radicand.extents().height() + vertical_gap + line_thickness);

        surd.origin.y = (radicand.extents().descent - surd.extents().descent) +
                        surd_excess_height / 2;

        // place the radicand after the surd
        radicand.origin.x += surd.origin.x + surd.advance_width();

        // the radical rule
        let origin = Point {
            x: surd.origin.x + surd.advance_width(),
            y: surd.origin.y - surd.extents().ascent + line_thickness / 2,
        };
        let target = Point { x: origin.x + radicand.extents().right_edge(), ..origin };
        let mut radical_rule = MathBox::with_line(origin, target, line_thickness as u32);

        let mut boxes = vec![];

        // typeset the self degree
        if !self.degree.is_empty() {
            let degree_bottom_raise_percent = PercentScale::new(shaper.math_constant(
                    MathConstant::RadicalDegreeBottomRaisePercent
            ) as u8);
            let kern_before = shaper.math_constant(MathConstant::RadicalKernBeforeDegree);
            let kern_after = shaper.math_constant(MathConstant::RadicalKernAfterDegree);
            let surd_height = surd.extents().ascent + surd.extents().descent;
            let degree_bottom = surd.origin.y + surd.extents().descent -
                                surd_height * degree_bottom_raise_percent;

            let mut degree_options = options;
            degree_options.style.script_level += 2;
            degree_options.style.math_style = MathStyle::Inline;
            let mut degree = self.degree.layout(degree_options);
            degree.origin.y += degree_bottom;
            degree.origin.x += kern_before;

            let surd_kern = kern_before + degree.advance_width() + kern_after;
            surd.origin.x += surd_kern;
            radicand.origin.x += surd_kern;
            radical_rule.origin.x += surd_kern;

            boxes.push(degree);
        }

        boxes.append(&mut vec![surd, radical_rule, radicand]);
        MathBox::with_vec(boxes)
        // TODO
        // let mut combined_box = boxes.into_iter().collect::<MathBox>();
        // combined_box.logical_extents.ascent += extra_ascender;
        // Box::new(iter::once(combined_box))
    }
}

impl Operator {
    fn layout_stretchy<'a, T: 'a + Debug>(self,
                                          needed_height: u32,
                                          needed_width: u32,
                                          options: LayoutOptions<'a>)
                                          -> MathBox<'a, T> {
        match self.field {
            Field::Unicode(ref string) => {
                let scale = options.shaper
                    .scale_factor_for_script_level(options.style.script_level);
                let needed_height = needed_height / scale;
                let needed_width = needed_width / scale;
                let mut shape_result = options.shaper.shape_string(string, options.style);
                let first_glyph = shape_result.next();

                if needed_width > 0 {
                    let stretched = first_glyph.and_then(move |shaped_glyph| {
                        options.shaper.stretch_glyph(shaped_glyph.glyph, true, options.as_accent, needed_width)
                    });
                    if let Some(stretched) = stretched {
                        return math_box_from_shaped_glyphs(stretched, options);
                    }
                }

                if needed_height > 0 {
                    let stretched = first_glyph.and_then(move |shaped_glyph| {
                        options.shaper.stretch_glyph(shaped_glyph.glyph, false, false, needed_height)
                    });
                    let mut math_box = match stretched {
                        Some(stretched) => math_box_from_shaped_glyphs(stretched, options),
                        // no stretched variant available, use the unstretched glyph
                        None => math_box_from_shaped_glyphs(first_glyph, options)
                    };
                    if let Some(stretch_constraints) = self.stretch_constraints {
                        if stretch_constraints.symmetric {
                            let axis_height = options.shaper
                                .math_constant(MathConstant::AxisHeight);
                            let shift_up =
                                (math_box.extents().descent - math_box.extents().ascent) / 2 +
                                axis_height;
                            math_box.origin.y -= shift_up;
                        } else {
                            let stretch_size = options.stretch_size.unwrap_or_default();
                            let excess_ascent = math_box.extents().ascent - stretch_size.ascent;
                            let excess_descent = math_box.extents().descent -
                                                 stretch_size.descent;
                            math_box.origin.y += (excess_ascent - excess_descent) / 2;
                        }
                    }

                    return math_box;
                }

                math_box_from_shaped_glyphs(first_glyph, options)
            }
            _ => unimplemented!(),
        }
    }
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for Operator {
    fn layout(mut self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        match (options.stretch_size, self.stretch_constraints) {
            (Some(stretch_size), Some(stretch_constraints)) => {
                let min_size = stretch_constraints.min_size
                    .map(|size| size.to_font_units(options.shaper));
                let max_size = stretch_constraints.max_size
                    .map(|size| size.to_font_units(options.shaper));
                let mut needed_height = if stretch_constraints.symmetric {
                    let axis_height = options.shaper.math_constant(MathConstant::AxisHeight);
                    max(stretch_size.ascent - axis_height,
                        axis_height + stretch_size.descent) * 2
                } else {
                    (stretch_size.ascent + stretch_size.descent)
                };
                needed_height = clamp(needed_height, min_size, max_size);
                let needed_height = max(0, needed_height) as u32;
                self.layout_stretchy(needed_height, stretch_size.width as u32, options)
            }
            _ => {
                if self.is_large_op && options.style.math_style == MathStyle::Display {
                    let display_min_height = options.shaper
                        .math_constant(MathConstant::DisplayOperatorMinHeight);
                    self.stretch_constraints = Some(StretchConstraints{ symmetric: true, ..Default::default() });
                    self.layout_stretchy(display_min_height as u32, 0, options)
                } else {
                    self.field.layout(options)
                }
            }
        }
    }

    fn operator_properties(&self, options: LayoutOptions<'a>) -> Option<OperatorProperties> {
        Some(OperatorProperties {
            stretch_properties: self.stretch_constraints.as_ref().map(|_| Default::default()),
            leading_space: self.leading_space.to_font_units(options.shaper),
            trailing_space: self.trailing_space.to_font_units(options.shaper),
        })
    }
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for MathSpace {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        let extents = Extents {
            left_side_bearing: 0,
            width: self.width.to_font_units(options.shaper),
            ascent: self.ascent.to_font_units(options.shaper),
            descent: self.descent.to_font_units(options.shaper),
        };
        MathBox::empty(extents)
    }
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for MathExpression<T> {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        let mut math_box = self.content.layout(options);
        math_box.user_info = Some(self.user_info);
        math_box
    }

    fn operator_properties(&self, options: LayoutOptions<'a>) -> Option<OperatorProperties> {
        self.content.operator_properties(options)
    }
}

impl<'a, T: 'a + Debug> MathBoxLayout<'a, T> for MathItem<T> {
    fn layout(self, options: LayoutOptions<'a>) -> MathBox<'a, T> {
        match self {
            MathItem::Field(field) => field.layout(options),
            MathItem::Space(space) => space.layout(options),
            MathItem::Atom(atom) => atom.layout(options),
            MathItem::GeneralizedFraction(frac) => frac.layout(options),
            MathItem::OverUnder(over_under) => over_under.layout(options),
            MathItem::List(list) => list.layout(options),
            MathItem::Root(root) => root.layout(options),
            MathItem::Operator(operator) => operator.layout(options),
        }
    }

    fn operator_properties(&self, options: LayoutOptions<'a>) -> Option<OperatorProperties> {
        match *self {
            MathItem::Field(ref field) => {
                MathBoxLayout::<'a, T>::operator_properties(field, options)
            }
            MathItem::Space(ref space) => {
                MathBoxLayout::<'a, T>::operator_properties(space, options)
            }
            MathItem::Atom(ref atom) => atom.operator_properties(options),
            MathItem::GeneralizedFraction(ref frac) => frac.operator_properties(options),
            MathItem::OverUnder(ref over_under) => over_under.operator_properties(options),
            MathItem::List(ref list) => list.operator_properties(options),
            MathItem::Root(ref root) => root.operator_properties(options),
            MathItem::Operator(ref operator) => {
                MathBoxLayout::<'a, T>::operator_properties(operator, options)
            }
        }
    }
}
