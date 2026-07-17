data modify storage mdl:list_work work set from storage mdl:list_work source
data modify storage mdl:list_work out set value []
return run function mdl_native:list/reverse_step
