execute if score #next mdl_hof matches 1001.. run return 0
execute store result storage mdl:hof generated int 1 run scoreboard players get #next mdl_hof
data modify storage mdl:hof source append from storage mdl:hof generated
scoreboard players add #next mdl_hof 1
return run function mdl_native:test/generate_1000
