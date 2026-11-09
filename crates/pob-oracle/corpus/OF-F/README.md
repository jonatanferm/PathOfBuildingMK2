# OF-F unit corpus

PvP variants of two slice5 builds (`corpus/U0/slice5/`), identical except for the config input
`PvpScaling = true` (ConfigOptions.lua:854), so the PvP branches of CalcOffence.lua 3867-3953
(`seg_passHit`) and 4031-4040 / 4075-4093 (`seg_combinePasses`) run:

| file | from | main skill |
|---|---|---|
| `of_f_pvp_allflame_exp_007.xml` | `slice5_allflame_exp_007` | Spark of the Nova (spell) |
| `of_f_pvp_allflame_exp_031.xml` | `slice5_allflame_exp_031` | Shield Crush (attack) |

Dumps: `scripts/port/oracle.sh run --build <file> --stage 'offence:2607*,offence:3346*,offence:3957*,offence:4155*'`.
