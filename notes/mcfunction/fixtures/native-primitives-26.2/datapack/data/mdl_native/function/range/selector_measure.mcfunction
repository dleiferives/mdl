execute store success storage mdl:observations ranges.selector.distance_exact byte 1 run execute in minecraft:overworld positioned 0.5 100 0.5 if entity @e[tag=mdl_range_near,distance=3..3]
execute store success storage mdl:observations ranges.selector.distance_closed byte 1 run execute in minecraft:overworld positioned 0.5 100 0.5 if entity @e[tag=mdl_range_near,distance=2.5..3.5]
execute store success storage mdl:observations ranges.selector.rotation_wrap_179 byte 1 run execute in minecraft:overworld positioned 0.5 100 0.5 if entity @e[tag=mdl_range_yaw_179,y_rotation=170..-170,distance=..1]
execute store success storage mdl:observations ranges.selector.rotation_wrap_zero byte 1 run execute in minecraft:overworld positioned 0.5 100 0.5 if entity @e[tag=mdl_range_yaw_zero,y_rotation=170..-170,distance=..1]
execute store success storage mdl:observations ranges.selector_calls.negative_distance byte 1 run function mdl_native:range/selector_distance_macro {label:"negative_distance",range:"-1..1"}
kill @e[tag=mdl_range_probe]
execute in minecraft:overworld run forceload remove 0 0
