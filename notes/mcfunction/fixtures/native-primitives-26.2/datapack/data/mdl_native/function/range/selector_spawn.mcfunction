kill @e[tag=mdl_range_probe]
execute in minecraft:overworld positioned 0.5 100 0.5 run summon item_display ~ ~ ~ {Tags:["mdl_range_probe","mdl_range_origin"]}
execute in minecraft:overworld positioned 0.5 100 0.5 run summon item_display ~3 ~ ~ {Tags:["mdl_range_probe","mdl_range_near"]}
execute in minecraft:overworld positioned 0.5 100 0.5 run summon item_display ~ ~ ~ {Tags:["mdl_range_probe","mdl_range_yaw_179"],Rotation:[179f,0f]}
execute in minecraft:overworld positioned 0.5 100 0.5 run summon item_display ~ ~ ~ {Tags:["mdl_range_probe","mdl_range_yaw_zero"],Rotation:[0f,0f]}
schedule function mdl_native:range/selector_measure 1t replace
