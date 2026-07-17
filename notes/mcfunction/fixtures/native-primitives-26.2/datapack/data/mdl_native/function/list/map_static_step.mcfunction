execute unless data storage mdl:hof work[0] run return 0
execute store result score #item mdl_hof run data get storage mdl:hof work[0]
scoreboard players operation #item mdl_hof *= #two mdl_hof
execute store result storage mdl:hof mapped int 1 run scoreboard players get #item mdl_hof
data modify storage mdl:hof out append from storage mdl:hof mapped
data remove storage mdl:hof work[0]
return run function mdl_native:list/map_static_step
