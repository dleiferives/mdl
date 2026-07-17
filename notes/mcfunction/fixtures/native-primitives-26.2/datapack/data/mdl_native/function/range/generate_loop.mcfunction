execute if score #step mdl_range matches 1.. unless score #current mdl_range < #stop mdl_range run return 1
execute if score #step mdl_range matches ..-1 unless score #current mdl_range > #stop mdl_range run return 1
execute store result storage mdl:observations ranges.generated.value int 1 run scoreboard players get #current mdl_range
data modify storage mdl:observations ranges.generated.current append from storage mdl:observations ranges.generated.value
scoreboard players operation #current mdl_range += #step mdl_range
function mdl_native:range/generate_loop
