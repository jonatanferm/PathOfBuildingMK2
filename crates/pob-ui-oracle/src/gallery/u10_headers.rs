//! Gallery "u10_headers" (U10): the Rust side of `lua/gallery/u10_headers.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_sg::Font;
use pob_ui_core::controls::{ButtonControl, LabelControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;
use pob_ui_core::tooltip::{Tooltip, TtBackground, TtColor, TtKey};

use super::_u9::Gallery;

struct State {
    page: Cell<i64>,
    tts: RefCell<Vec<Tooltip>>,
}

fn cc(cx: &Cx, name: &str) -> String {
    cx.main.cc.get(name).cloned().expect("colour code")
}

fn line(cx: &Cx, tt: &mut Tooltip, size: f64, text: &str) {
    tt.add_line_ex(cx, size, text, Some(Font::FontinSc), None, None);
}

fn item_lines(cx: &Cx, tt: &mut Tooltip, rarity: &str, name: &str) {
    let c = cc(cx, rarity);
    line(cx, tt, 20.0, &format!("{c}{name}"));
    line(cx, tt, 20.0, &format!("{c}Leather Belt"));
    tt.add_separator(cx, Some(10.0));
    line(cx, tt, 16.0, "^x7F7F7FRequires Level ^768");
    tt.add_separator(cx, Some(10.0));
    let m = cc(cx, "MAGIC");
    line(cx, tt, 16.0, &format!("{m}+30 to maximum Life"));
    line(cx, tt, 16.0, &format!("{m}+12% to Fire Resistance"));
}

fn overflow_lines(cx: &Cx, tt: &mut Tooltip, label: &str) {
    tt.center = true;
    tt.add_line(cx, 16.0, &format!("^7{label} overflow tooltip"));
    for b in 1..=9 {
        tt.add_line(cx, 16.0, &format!("^7Equipping block {b} will give you:"));
        for l in 1..=6 {
            let extra = if l == 3 { " (a longer line here)" } else { "" };
            tt.add_line(cx, 14.0, &format!("^x88AAFFline {l} of block {b}{extra}"));
        }
        tt.add_separator(cx, Some(8.0));
        tt.add_line(cx, 14.0, "^7Removing it will give you:");
        tt.add_line(cx, 14.0, "^1-10 to something");
    }
}

fn header(tt: &mut Tooltip, h: &str) {
    tt.tooltip_header = Some(h.to_owned());
}

fn build(cx: &Cx, page: i64, i: usize, tt: &mut Tooltip) {
    tt.influence_header1 = None;
    tt.influence_header2 = None;
    tt.child_tooltips = None;
    let esc = |name: &str| Some(TtColor::Esc(cc(cx, name)));
    match (page, i) {
        (1, 1) => {
            header(tt, "UNIQUE");
            tt.influence_header1 = Some("Shaper".into());
            tt.influence_header2 = Some("Elder".into());
            tt.center = true;
            tt.color = esc("UNIQUE");
            tt.max_width = Some(360.0);
            item_lines(cx, tt, "UNIQUE", "Headhunter");
            tt.add_separator(cx, Some(10.0));
            tt.add_line_ex(
                cx,
                16.0,
                &format!(
                    "{}Memory Strands: ^7 12 and some words so that it wraps onto more lines",
                    cc(cx, "MEMORY")
                ),
                Some(Font::FontinSc),
                None,
                Some(TtBackground::Image("Assets/memorybg.png".into())),
            );
            tt.add_line_ex(
                cx,
                16.0,
                &format!(
                    "{}Intangibility: ^7 5% and also enough words to wrap a few times",
                    cc(cx, "INTANGIBILITY")
                ),
                Some(Font::FontinSc),
                None,
                Some(TtBackground::Image("Assets/intangibilitybg.png".into())),
            );
            line(cx, tt, 16.0, "after the backgrounds");
        }
        (1, 2) => {
            header(tt, "RARE");
            tt.influence_header1 = Some("Hunter".into());
            tt.influence_header2 = Some("Hunter".into());
            tt.center = true;
            tt.color = esc("RARE");
            item_lines(cx, tt, "RARE", "Doom Cord");
        }
        (1, 3) => {
            header(tt, "rare");
            tt.color = Some(TtColor::Rgb([0.2, 0.4, 0.9]));
            item_lines(cx, tt, "RARE", "Lower-case header");
        }
        (2, 1) => {
            header(tt, "MAGIC");
            tt.center = true;
            tt.color = esc("MAGIC");
            item_lines(cx, tt, "MAGIC", "Sapphire Ring of the Whale");
        }
        (2, 2) => {
            header(tt, "NORMAL");
            tt.influence_header1 = Some("Crusader".into());
            tt.influence_header2 = Some("Warlord".into());
            tt.color = esc("NORMAL");
            item_lines(cx, tt, "NORMAL", "Iron Ring");
        }
        (2, 3) => {
            header(tt, "GEM");
            tt.center = true;
            tt.color = esc("GEM");
            let g = cc(cx, "GEM");
            line(cx, tt, 20.0, &format!("{g}Fireball"));
            tt.add_separator(cx, Some(10.0));
            line(cx, tt, 16.0, "^x7F7F7FLevel: ^720");
            tt.add_separator(cx, Some(10.0));
            tt.add_line_ex(
                cx,
                16.0,
                &format!("{g}Unleashes a ball of fire."),
                Some(Font::FontinSc),
                None,
                Some(TtBackground::Image("Assets/memorybg.png".into())),
            );
        }
        (2, 4) => {
            header(tt, "RELIC");
            tt.foil_type = Some("Verdant".into());
            tt.center = true;
            item_lines(cx, tt, "RELIC", "Relic belt");
        }
        (2, 5) => {
            header(tt, "RELIC");
            tt.foil_type = Some("Not a foil".into());
            tt.influence_header1 = Some("Synthesis".into());
            tt.influence_header2 = Some("Synthesis".into());
            item_lines(cx, tt, "RELIC", "Unknown foil");
        }
        (3, i) => {
            const HEADERS: [&str; 8] = [
                "NOTABLE",
                "KEYSTONE",
                "ASCENDANCY",
                "MASTERY",
                "MASTERYALLOC",
                "JEWEL",
                "PASSIVE",
                "UNKNOWN",
            ];
            let h = HEADERS[i - 1];
            header(tt, h);
            tt.center = i % 2 == 1;
            if i == 1 {
                tt.set_recipe(Some(vec![
                    "GoldenOil".into(),
                    "SilverOil".into(),
                    "OpalescentOil".into(),
                ]));
            } else if i == 2 {
                tt.set_recipe(Some(vec![
                    "AmberOil".into(),
                    "CrimsonOil".into(),
                    "TealOil".into(),
                    "VioletOil".into(),
                ]));
            }
            line(
                cx,
                tt,
                18.0,
                &format!("^7{} node", pob_lua::string::lower(h)),
            );
            tt.add_separator(cx, Some(10.0));
            tt.add_line(cx, 16.0, "^x8888FF10% increased Damage");
            tt.add_line(cx, 16.0, "^x8888FF+5 to Strength");
        }
        (4, 1) => overflow_lines(cx, tt, "Static"),
        (4, 2) => {
            header(tt, "UNIQUE");
            tt.center = true;
            item_lines(cx, tt, "UNIQUE", "Parent with children");
            let mut children = vec![Tooltip::default(), Tooltip::default()];
            for (k0, child) in children.iter_mut().enumerate() {
                let k = k0 + 1;
                header(child, "GEM");
                child.center = true;
                child.color = esc("GEM");
                line(cx, child, 18.0, &format!("{}Child gem {k}", cc(cx, "GEM")));
                child.add_separator(cx, Some(10.0));
                for l in 1..=8 {
                    child.add_line(cx, 14.0, &format!("^7child line {l}"));
                }
            }
            tt.child_tooltips = Some(children);
        }
        _ => {}
    }
}

/// where page p draws tooltip i
fn positions(page: i64) -> &'static [(f64, f64)] {
    match page {
        1 => &[(20.0, 50.0), (420.0, 50.0), (820.0, 50.0)],
        2 => &[
            (20.0, 50.0),
            (350.0, 50.0),
            (700.0, 50.0),
            (20.0, 400.0),
            (400.0, 400.0),
        ],
        3 => &[
            (20.0, 50.0),
            (320.0, 50.0),
            (620.0, 50.0),
            (920.0, 50.0),
            (20.0, 330.0),
            (320.0, 330.0),
            (620.0, 330.0),
            (920.0, 330.0),
        ],
        _ => &[(0.0, 40.0), (900.0, 300.0)],
    }
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    let s = Rc::new(State {
        page: Cell::new(1),
        tts: RefCell::new(Vec::new()),
    });
    let s0 = s.clone();
    Gallery::new(
        parts,
        move |cx, host| {
            let s = s0;
            // the first ButtonControl loads Classes/Tooltip.lua
            module_load(cx);
            let st = s.clone();
            let next = cx.add(
                host,
                "next",
                ButtonControl::new(Anchor::NONE, rect(10, 10, 80, 20), "Next", move |_| {
                    st.page.set(pob_lua::fmod_lua(st.page.get() as f64, 4.0) as i64 + 1)
                }),
            );
            let flavour = cx.add(
                host,
                "flavour",
                ButtonControl::new(
                    Anchor::new(Left, next, Right),
                    rect(10, 0, 80, 20),
                    "Flavour",
                    |cx| cx.main.show_flavour_text = !cx.main.show_flavour_text,
                ),
            );
            let st = s.clone();
            cx.add(
                host,
                "page",
                LabelControl::new(
                    Anchor::new(Left, flavour, Right),
                    rect(10, 0, 0, 16),
                    Prop::f(move |cx, _| {
                        format!(
                            "^7page {}  flavour {}",
                            st.page.get(),
                            cx.main.show_flavour_text
                        )
                    }),
                ),
            );
            let overflow = cx.add(
                host,
                "overflow",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(Prop::f(|cx, _| cx.main.screen_w - 90.0), 10, 80, 20),
                    "Overflow",
                    |_| {},
                ),
            );
            cx.get_mut(overflow).tt.tooltip_func = Some(Rc::new(|cx, tt, ()| {
                if tt.check_for_update(cx, &[TtKey::Bool(cx.main.show_flavour_text)]) {
                    overflow_lines(cx, tt, "Hover");
                }
            }));
            *s.tts.borrow_mut() = (0..8).map(|_| Tooltip::default()).collect();
        },
        move |cx, host, ev, vp| {
            cx.process_controls_input(host, ev, vp);
            cx.sg.set_draw_color(0.1, 0.1, 0.1);
            cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
            let page = s.page.get();
            let mut tts = s.tts.borrow_mut();
            for (i0, &(x, y)) in positions(page).iter().enumerate() {
                let tt = &mut tts[i0];
                let key = [
                    TtKey::Num(page as f64),
                    TtKey::Bool(cx.main.show_flavour_text),
                ];
                if tt.check_for_update(cx, &key) {
                    build(cx, page, i0 + 1, tt);
                }
                tt.draw(cx, x, y, None, None, vp);
            }
            drop(tts);
            cx.draw_controls(host, vp, None);
        },
    )
}
