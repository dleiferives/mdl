kill @e[tag=mdl_float_alu]
execute in minecraft:overworld positioned 0.5 100 0.5 run summon item_display ~ ~ ~ {Tags:["mdl_float_alu"]}
schedule function mdl_native:float/entity_alu 1t replace
