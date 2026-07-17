execute unless data storage mdl:hof work[0] run return 0
execute store result score #item mdl_hof run data get storage mdl:hof work[0]
scoreboard players operation #remainder mdl_hof = #item mdl_hof
scoreboard players operation #remainder mdl_hof %= #two mdl_hof
execute if score #remainder mdl_hof matches 0 run data modify storage mdl:hof out append from storage mdl:hof work[0]
data remove storage mdl:hof work[0]
return run function mdl_native:list/filter_even_step
