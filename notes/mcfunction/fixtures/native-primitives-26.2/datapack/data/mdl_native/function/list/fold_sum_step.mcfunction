execute unless data storage mdl:hof work[0] run return run scoreboard players get #acc mdl_hof
execute store result score #item mdl_hof run data get storage mdl:hof work[0]
scoreboard players operation #acc mdl_hof += #item mdl_hof
data remove storage mdl:hof work[0]
return run function mdl_native:list/fold_sum_step
