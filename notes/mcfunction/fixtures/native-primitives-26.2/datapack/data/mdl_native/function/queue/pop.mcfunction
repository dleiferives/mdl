execute unless data storage mdl:queue out[-1] if data storage mdl:queue in[-1] run function mdl_native:queue/refill
execute unless data storage mdl:queue out[-1] run return 0
data modify storage mdl:queue output set from storage mdl:queue out[-1]
data remove storage mdl:queue out[-1]
return 1
