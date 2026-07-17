execute unless data storage mdl:list_work work[0] run return 0
data modify storage mdl:list_work out append from storage mdl:list_work work[-1]
data remove storage mdl:list_work work[-1]
return run function mdl_native:list/reverse_step
