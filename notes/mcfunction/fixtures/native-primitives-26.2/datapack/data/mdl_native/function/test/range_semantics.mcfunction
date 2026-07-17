data remove storage mdl:observations ranges
scoreboard objectives remove mdl_range
scoreboard objectives add mdl_range dummy

# IntegerRange syntax is inclusive. Exact values and either omitted bound are native.
scoreboard players set #value mdl_range 5
execute store success storage mdl:observations ranges.score.exact byte 1 run execute if score #value mdl_range matches 5
execute store success storage mdl:observations ranges.score.closed_lower byte 1 run execute if score #value mdl_range matches 5..8
execute store success storage mdl:observations ranges.score.closed_upper byte 1 run execute if score #value mdl_range matches 2..5
execute store success storage mdl:observations ranges.score.lower_unbounded byte 1 run execute if score #value mdl_range matches ..5
execute store success storage mdl:observations ranges.score.upper_unbounded byte 1 run execute if score #value mdl_range matches 5..
execute store success storage mdl:observations ranges.score.outside byte 1 run execute if score #value mdl_range matches 6..8
scoreboard players set #value mdl_range -2147483648
execute store success storage mdl:observations ranges.score.int_min byte 1 run execute if score #value mdl_range matches -2147483648..-2147483648
scoreboard players set #value mdl_range 2147483647
execute store success storage mdl:observations ranges.score.int_max byte 1 run execute if score #value mdl_range matches 2147483647..2147483647
scoreboard players set #value mdl_range 5

# Intersection is conjunction. Complement is `unless`. Union needs branches/a flag.
execute store success storage mdl:observations ranges.algebra.intersection byte 1 run execute if score #value mdl_range matches 3.. if score #value mdl_range matches ..7
execute store success storage mdl:observations ranges.algebra.complement byte 1 run execute unless score #value mdl_range matches 6..8
scoreboard players set #union mdl_range 0
execute if score #value mdl_range matches ..2 run scoreboard players set #union mdl_range 1
execute if score #value mdl_range matches 5..7 run scoreboard players set #union mdl_range 1
execute store result storage mdl:observations ranges.algebra.union int 1 run scoreboard players get #union mdl_range

# Runtime endpoints compose as score comparisons without reparsing command text.
scoreboard players set #lower mdl_range 3
scoreboard players set #upper mdl_range 7
execute store success storage mdl:observations ranges.algebra.dynamic_closed byte 1 run execute if score #value mdl_range >= #lower mdl_range if score #value mdl_range <= #upper mdl_range

# Macro probes preserve parser failures without making the pack fail to load.
function mdl_native:range/matches_macro {label:"closed",range:"5..8"}
execute store success storage mdl:observations ranges.macro_calls.inverted byte 1 run function mdl_native:range/matches_macro {label:"inverted",range:"8..5"}
execute store success storage mdl:observations ranges.macro_calls.missing_both byte 1 run function mdl_native:range/matches_macro {label:"missing_both",range:".."}

# The random command consumes the same IntegerRange argument and includes both ends.
execute store success storage mdl:observations ranges.random.singleton_success byte 1 store result storage mdl:observations ranges.random.singleton int 1 run random value 3..3
execute store result score #random mdl_range run random value -2..2
execute store result storage mdl:observations ranges.random.sample int 1 run scoreboard players get #random mdl_range
execute store success storage mdl:observations ranges.random.sample_in_range byte 1 run execute if score #random mdl_range matches -2..2

# A progression is not a membership interval: it has a direction, step and exclusive stop.
function mdl_native:range/generate {start:0,stop:5,step:2}
data modify storage mdl:observations ranges.generated.ascending set from storage mdl:observations ranges.generated.current
function mdl_native:range/generate {start:5,stop:0,step:-2}
data modify storage mdl:observations ranges.generated.descending set from storage mdl:observations ranges.generated.current
execute store success storage mdl:observations ranges.generated.empty_success byte 1 run function mdl_native:range/generate {start:3,stop:3,step:1}
data modify storage mdl:observations ranges.generated.empty set from storage mdl:observations ranges.generated.current
execute store success storage mdl:observations ranges.generated.zero_step_success byte 1 run function mdl_native:range/generate {start:0,stop:5,step:0}

# Entity probes run one tick after force-loading their isolated chunk.
execute in minecraft:overworld run forceload add 0 0
schedule function mdl_native:range/selector_spawn 1t replace

# Stopwatch ranges are the command language's native floating-point interval.
stopwatch remove mdl_native:range_probe
stopwatch create mdl_native:range_probe
execute store success storage mdl:observations ranges.stopwatch.broad byte 1 run execute if stopwatch mdl_native:range_probe ..1000
execute store result storage mdl:observations ranges.stopwatch.elapsed_ms int 1 run stopwatch query mdl_native:range_probe 1000
function mdl_native:range/stopwatch_macro {label:"open",range:"0.0.."}
execute store success storage mdl:observations ranges.stopwatch_calls.inverted byte 1 run function mdl_native:range/stopwatch_macro {label:"inverted",range:"2.0..1.0"}
execute store success storage mdl:observations ranges.stopwatch_calls.missing_both byte 1 run function mdl_native:range/stopwatch_macro {label:"missing_both",range:".."}
stopwatch remove mdl_native:range_probe

data get storage mdl:observations ranges
scoreboard objectives remove mdl_range
