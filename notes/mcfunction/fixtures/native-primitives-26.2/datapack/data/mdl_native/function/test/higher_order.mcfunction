scoreboard objectives add mdl_hof dummy
scoreboard players set #two mdl_hof 2
data modify storage mdl:hof source set value [1,2,3,4]
function mdl_native:list/map_static
data modify storage mdl:hof results.static_map set from storage mdl:hof out
data modify storage mdl:hof config set value {callback:"mdl_native:callback/square"}
function mdl_native:list/map_dynamic
data modify storage mdl:hof results.dynamic_map set from storage mdl:hof out
function mdl_native:list/filter_even
data modify storage mdl:hof results.filter_even set from storage mdl:hof out
function mdl_native:list/fold_sum
execute store result storage mdl:hof results.sum int 1 run scoreboard players get #acc mdl_hof
data modify storage mdl:hof dict set value {alpha:11,beta:22,"space key":33}
function mdl_native:dynamic/pick {index:2}
data modify storage mdl:hof results.picked_index set from storage mdl:hof picked
function mdl_native:dynamic/get_key {key:beta}
data modify storage mdl:hof results.picked_key set from storage mdl:hof picked
function mdl_native:dynamic/get_key {key:'"space key"'}
data modify storage mdl:hof results.picked_quoted_key set from storage mdl:hof picked
scoreboard players set #item mdl_hof 5
execute store success score #tag_success mdl_hof store result score #tag_result mdl_hof run function #mdl_native:callbacks
execute store result storage mdl:hof results.tag_success int 1 run scoreboard players get #tag_success mdl_hof
execute store result storage mdl:hof results.tag_result int 1 run scoreboard players get #tag_result mdl_hof
data get storage mdl:hof results
