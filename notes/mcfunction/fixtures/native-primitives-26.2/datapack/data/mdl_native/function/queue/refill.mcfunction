execute unless data storage mdl:queue in[-1] run return 0
data modify storage mdl:queue out append from storage mdl:queue in[-1]
data remove storage mdl:queue in[-1]
return run function mdl_native:queue/refill
