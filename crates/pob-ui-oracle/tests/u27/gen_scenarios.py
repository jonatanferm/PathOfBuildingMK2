"""Writes the u27 scenario scripts (crates/pob-ui-oracle/tests/u27_timeless.rs), owner U27.

Usage: python3 gen_scenarios.py <tests/u27 dir> [scenario ...]

Each scenario is written twice: `<name>.pobscript` at 1280x800 and `<name>_1920.pobscript` at
1920x1080 @1.5 (virtual 1280x720: the popup sits 40 px higher, the TreeTab's bottom line 80 px).
`{yN}` in a body is a 1280x800 y coordinate. Popup coordinates (1280x800): the popup at
(122, 106) 1036x588; Search (550, 672), Reset (640, 672), Cancel (730, 672); Open Trade URL
(1061, 434); Desired / Fallback Nodes (187 / 300, 434); Generate (880, 346); the node weight
sliders at y 254 / 277 / 300 (x 642-842); the jewel type / node dropdowns (742, 139 / 323).
"""
import os
import re
import sys

d = sys.argv[1]
OPEN = """fixture userdir=user
fixture net=net.json
frames 2
load-build "Builds/{build}"
frames 3
move 699 {fy}
frames 1
click LEFTBUTTON
frames 3
"""
scen = {}
scen['a_lp'] = ("lp.xml", """# U27 search 1: Lethal Pride on the Ghost Dance socket (61834) with four desired notables (the build's
# TimelessData): open the popup (idle layout; the PC league list arrives from the net fixture), hover the
# primary node weight slider (its tooltip.Draw override puts the tooltip above the knob), Search (results sorted
# by total), Open Trade URL (OpenURL + Copy of the URL), Open Next Trade URL, then the build XML.
#! u27-expect-text   17570:     1.0     14.0       0          3.0
#! u27-expect-text Open Next Trade URL
#! u27-expect-effect OpenUrl
#! u27-expect-text ^7For nodes with multiple stats this slider controls the weight of the first stat listed.
""", """move 662 {y254}
frames 2
move 550 {y672}
frames 1
click LEFTBUTTON
frames 2
move 1061 {y434}
frames 1
click LEFTBUTTON
frames 2
click LEFTBUTTON
frames 2
move 560 {y400}
frames 1
snapshot lp
frames 1
""")
scen['b_gv'] = ("gv.xml", """# U27 search 2: Glorious Vanity (Doryani) on 61834: two notables with secondary weights and the small life
# node (GV scores every radius node, headers of 2/3 and 6/8 entries), one fallback row; Search, show the
# fallback list and back, hover the secondary weight slider, then the build XML.
#! u27-expect-text 1370:   37.5   307.0       1.25   28.0
#! u27-expect-text ^7For nodes with multiple stats this slider controls the weight of the second stat listed.
""", """move 550 {y672}
frames 1
click LEFTBUTTON
frames 2
move 300 {y434}
frames 1
click LEFTBUTTON
frames 2
move 187 {y434}
frames 1
click LEFTBUTTON
frames 2
move 662 {y277}
frames 2
move 560 {y400}
frames 1
snapshot gv
frames 1
""")
scen['c_mf'] = ("mf.xml", """# U27 search 3: Militant Faith (Dominus, devotion variants 2 and 6) on 61834 with the node filter on
# (distance 4: the distance slider and the protect check boxes of the allocated notables in radius), Total
# Devotion plus two notables; protect the first notable, Search, Open Trade URL (the devotion filters of the
# two variants), then the build XML.
#! u27-expect-text ^7Protect allocated nodes from changing:
#! u27-expect-text ^7Node Distance:
#! u27-expect-text   2424: 370.0
#! u27-expect-effect OpenUrl
""", """move 145 {y160}
frames 1
click LEFTBUTTON
frames 2
move 550 {y672}
frames 1
click LEFTBUTTON
frames 2
move 1061 {y434}
frames 1
click LEFTBUTTON
frames 2
move 560 {y400}
frames 1
snapshot mf
frames 1
""")
scen['d_eh'] = ("eh.xml", """# U27 search 4: Elegant Hubris on All Sockets (the multi-socket search: every jewel socket, the results
# labelled by socket and sorted by total, then socket label) with three notables (one with a minimum weight);
# Search, then the build XML.
#! u27-expect-text Duelist: 54127 | 136000:
""", """move 550 {y672}
frames 1
click LEFTBUTTON
frames 2
move 560 {y400}
frames 1
snapshot eh
frames 1
""")
scen['e_abyss'] = ("abyss.xml", """# U27 search 5: Festering Vengeance (an abyss LUT: the affected passives come from readAbyssJewelLUT, the
# rolls scale the weights) on 61834 with three notables; Search, Open Trade URL (the abyss keystone trade id),
# then the build XML.
#! u27-expect-text 2405:   95.0     58.0
#! u27-expect-effect OpenUrl
""", """move 550 {y672}
frames 1
click LEFTBUTTON
frames 2
move 1061 {y434}
frames 1
click LEFTBUTTON
frames 2
move 560 {y400}
frames 1
snapshot abyss
frames 1
""")
scen['g_generate'] = ("lp.xml", """# U27 fallback weights: Lethal Pride on 61834; Generate (setupFallbackWeights: every node of the list through
# the misc calculator, "Sort by Full DPS"; the Rust side's misc calculator is the Rust calc, CCR-0132 §4), show
# the fallback list, hover Generate (its tooltip), Generate again, Search (the fallback rows join the desired
# ones), then the build XML.
#! u27-rust-misc-calc
#! u27-expect-text ^4Fallback Nodes
#! u27-expect-text ^7Click this button to generate new fallback node weights
#! u27-expect-text karui_notable_add_double_damage, 4.364, 0, 0
""", """move 880 {y346}
frames 1
click LEFTBUTTON
frames 2
move 300 {y434}
frames 1
click LEFTBUTTON
frames 2
move 880 {y346}
frames 2
click LEFTBUTTON
frames 2
move 550 {y672}
frames 1
click LEFTBUTTON
frames 2
move 560 {y400}
frames 1
snapshot generate
frames 1
""")
scen['h_generate_gv'] = ("gv.xml", """# U27 fallback weights with variable rolls: Glorious Vanity on 61834; Generate (the multi-stat nodes get one
# describeStats line and mod list per stat, buildStatModLists, cached in the legion tables as
# modListGenerated), show the fallback list, Generate again (the cached lists), then the build XML.
#! u27-rust-misc-calc
#! u27-expect-text ^4Fallback Nodes
#! u27-expect-text vaal_notable_physical_damage_1, 0.873, 0.083, 0
""", """move 880 {y346}
frames 1
click LEFTBUTTON
frames 2
move 300 {y434}
frames 1
click LEFTBUTTON
frames 2
move 880 {y346}
frames 1
click LEFTBUTTON
frames 2
move 560 {y400}
frames 1
snapshot generate_gv
frames 1
""")
scen['f_controls'] = ("timeless_base.xml", """# U27 controls: open the popup on a build without timeless data (Glorious Vanity, All Sockets), drop the jewel
# type list and pick Lethal Pride (buildMods, the conqueror list), drop the node list and pick Total Strength
# (a new desired row), click the primary weight slider's track (the row's weights follow), hover the minimum
# weight slider (its tooltip override), show the fallback list, Reset, Cancel, then the build XML.
#! u27-expect-text total_strength, 5, 0, 0
#! u27-expect-text ^7Seeds that do not meet the minimum weight threshold
#! u27-expect-text Lethal Pride
""", """move 742 {y139}
frames 1
click LEFTBUTTON
frames 2
move 742 {y167}
frames 1
click LEFTBUTTON
frames 2
move 742 {y323}
frames 1
click LEFTBUTTON
frames 2
move 742 {y351}
frames 1
click LEFTBUTTON
frames 2
move 742 {y254}
frames 1
click LEFTBUTTON
frames 2
move 742 {y300}
frames 2
move 300 {y434}
frames 1
click LEFTBUTTON
frames 2
move 640 {y672}
frames 1
click LEFTBUTTON
frames 2
move 730 {y672}
frames 1
click LEFTBUTTON
frames 2
snapshot controls
frames 1
""")
only = sys.argv[2:] or list(scen)
for name, (build, head, body) in scen.items():
    if name not in only:
        continue
    for tag, scr, dy, fy in (("", "screen 1280 800", 0, 786), ("_1920", "screen 1920 1080 dpi 1.5", -40, 706)):
        b = re.sub(r"\{y(\d+)\}", lambda m: str(int(m.group(1)) + dy), body)
        text = head + scr + "\n" + OPEN.format(build=build, fy=fy) + b
        open(os.path.join(d, name + tag + ".pobscript"), "w").write(text)
