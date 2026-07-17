execute unless data storage mdl:hof work[0] run return 0
execute store result score #item mdl_hof run data get storage mdl:hof work[0]
$execute store result score #mapped mdl_hof run function $(callback)
execute store result storage mdl:hof mapped int 1 run scoreboard players get #mapped mdl_hof
data modify storage mdl:hof out append from storage mdl:hof mapped
data remove storage mdl:hof work[0]
return run function mdl_native:list/map_dynamic_step with storage mdl:hof config
