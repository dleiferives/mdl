data modify entity @n[type=item_display,tag=mdl_float_alu] transformation set value [1f,0f,0f,7f,0f,1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,2f]
data modify storage mdl:observations floats.entity_alu.seven_over_two set from entity @n[type=item_display,tag=mdl_float_alu] transformation.translation[0]
data modify entity @n[type=item_display,tag=mdl_float_alu] transformation set value [1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,4f]
data modify storage mdl:observations floats.entity_alu.reciprocal_four set from entity @n[type=item_display,tag=mdl_float_alu] transformation.scale[0]
data modify entity @n[type=item_display,tag=mdl_float_alu] transformation set value [1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,0f]
data modify storage mdl:observations floats.entity_alu.nan set from entity @n[type=item_display,tag=mdl_float_alu] transformation.translation[0]
data modify entity @n[type=item_display,tag=mdl_float_alu] transformation set value [1f,0f,0f,1f,0f,1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,0f]
data modify storage mdl:observations floats.entity_alu.infinity set from entity @n[type=item_display,tag=mdl_float_alu] transformation.translation[0]
data modify entity @n[type=item_display,tag=mdl_float_alu] transformation set value [1f,0f,0f,-1f,0f,1f,0f,0f,0f,0f,1f,0f,0f,0f,0f,0f]
data modify storage mdl:observations floats.entity_alu.negative_infinity set from entity @n[type=item_display,tag=mdl_float_alu] transformation.translation[0]
execute store result storage mdl:observations floats.entity_alu.nan_as_result int 1 run data get storage mdl:observations floats.entity_alu.nan
execute store result storage mdl:observations floats.entity_alu.infinity_as_result int 1 run data get storage mdl:observations floats.entity_alu.infinity
execute store result storage mdl:observations floats.entity_alu.negative_infinity_as_result int 1 run data get storage mdl:observations floats.entity_alu.negative_infinity
kill @e[tag=mdl_float_alu]
execute in minecraft:overworld run forceload remove 0 0
