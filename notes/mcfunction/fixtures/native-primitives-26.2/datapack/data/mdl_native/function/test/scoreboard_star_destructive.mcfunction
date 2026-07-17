data remove storage mdl:observations scoreboard_star
scoreboard players reset *
scoreboard objectives remove mdl.star.a
scoreboard objectives remove mdl.star.b
scoreboard objectives remove mdl.star.src
scoreboard objectives remove mdl.star.dst
scoreboard objectives remove mdl.star.alias
scoreboard objectives add mdl.star.a dummy
scoreboard objectives add mdl.star.b dummy
scoreboard objectives add mdl.star.src dummy
scoreboard objectives add mdl.star.dst dummy
scoreboard objectives add mdl.star.alias dummy
execute store success storage mdl:observations scoreboard_star.empty_success byte 1 store result storage mdl:observations scoreboard_star.empty_result int 1 run scoreboard players add * mdl.star.a 1
scoreboard players set #grp.alpha mdl.star.a 1
scoreboard players set #grp.beta mdl.star.a 2
scoreboard players set #other mdl.star.a 3
scoreboard players set #grp* mdl.star.a 4
scoreboard players set #grp? mdl.star.a 5
scoreboard players set #grp[ab] mdl.star.a 6
scoreboard players set #grp.* mdl.star.a 7
scoreboard players add #grp* mdl.star.a 100
scoreboard players add #grp.* mdl.star.a 1000
execute store result storage mdl:observations scoreboard_star.literal_star_name int 1 run scoreboard players get #grp* mdl.star.a
execute store result storage mdl:observations scoreboard_star.literal_regex_name int 1 run scoreboard players get #grp.* mdl.star.a
execute store result storage mdl:observations scoreboard_star.tracked_before_broadcast int 1 run scoreboard players list
execute store result storage mdl:observations scoreboard_star.broadcast_set_result int 1 run scoreboard players set * mdl.star.b 10
scoreboard players set #grp.alpha mdl.star.src 2
scoreboard players set #grp.beta mdl.star.src 3
scoreboard players set #acc mdl.star.dst 100
scoreboard players set #acc mdl.star.src 0
execute store result storage mdl:observations scoreboard_star.reduce_result int 1 run scoreboard players operation #acc mdl.star.dst += * mdl.star.src
execute store result storage mdl:observations scoreboard_star.reduced_value int 1 run scoreboard players get #acc mdl.star.dst
execute store result score * mdl.star.dst run scoreboard players set #new mdl.star.src 5
execute store result storage mdl:observations scoreboard_star.preexisting_stored_value int 1 run scoreboard players get #grp.alpha mdl.star.dst
execute store success storage mdl:observations scoreboard_star.new_holder_received_store byte 1 run scoreboard players get #new mdl.star.dst
scoreboard players reset *
scoreboard players set #a mdl.star.alias 1
scoreboard players set #b mdl.star.alias 2
execute store result storage mdl:observations scoreboard_star.alias_result int 1 run scoreboard players operation * mdl.star.alias += * mdl.star.alias
execute store result storage mdl:observations scoreboard_star.alias_a int 1 run scoreboard players get #a mdl.star.alias
execute store result storage mdl:observations scoreboard_star.alias_b int 1 run scoreboard players get #b mdl.star.alias
scoreboard players reset *
scoreboard players set #a mdl.star.dst 100
scoreboard players set #a mdl.star.src 2
scoreboard players set #b mdl.star.src 0
execute store success storage mdl:observations scoreboard_star.partial_success byte 1 store result storage mdl:observations scoreboard_star.partial_result int 1 run scoreboard players operation #a mdl.star.dst /= * mdl.star.src
execute store result storage mdl:observations scoreboard_star.partial_value int 1 run scoreboard players get #a mdl.star.dst
data get storage mdl:observations scoreboard_star
