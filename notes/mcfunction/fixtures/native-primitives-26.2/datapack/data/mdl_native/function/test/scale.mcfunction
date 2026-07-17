scoreboard objectives add mdl_hof dummy
scoreboard players set #two mdl_hof 2
scoreboard players set #next mdl_hof 1
data modify storage mdl:hof source set value []
function mdl_native:test/generate_1000
return run function mdl_native:list/map_static
