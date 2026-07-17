$scoreboard players set #current mdl_range $(start)
$scoreboard players set #stop mdl_range $(stop)
$scoreboard players set #step mdl_range $(step)
data modify storage mdl:observations ranges.generated.current set value []
execute if score #step mdl_range matches 0 run return fail
function mdl_native:range/generate_loop
return 1
