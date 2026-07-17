data modify storage mdl:hof work set from storage mdl:hof source
data modify storage mdl:hof out set value []
return run function mdl_native:list/map_dynamic_step with storage mdl:hof config
