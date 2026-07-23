#tellraw @a [{score:{name:{cmd:"#ptr",objective:{cmd:"brainfuck.re"}}},{text:{cmd:" "}},{storage:{cmd:"brainfuck:re",nbt:{cmd:"ptr"}}]

#execute if data storage brainfuck:re {cur_char:{cmd:"+"}} run say plus
#execute if data storage brainfuck:re {cur_char:{cmd:"-"}} run say minus
#execute if data storage brainfuck:re {cur_char:{cmd:"<"}} run say ls
#execute if data storage brainfuck:re {cur_char:{cmd:">"}} run say rs
#execute if data storage brainfuck:re {cur_char:{cmd:"."}} run say out
#execute if data storage brainfuck:re {cur_char:{cmd:","}} run say in
#execute if data storage brainfuck:re {cur_char:{cmd:"["}} run say open
#execute if data storage brainfuck:re {cur_char:{cmd:"]"}} run say close

#data modify storage brainfuck:re cur_char.cmd set from storage brainfuck:re cur_char.list.cmd
execute store result score #value brainfuck.re run data get storage brainfuck:re cur_char.value

#tellraw @a {storage:"brainfuck:re",nbt:"cur_char"}

execute if data storage brainfuck:re {cur_char:{cmd:"add"}} run return run function brainfuck:compile/process/plus/
execute if data storage brainfuck:re {cur_char:{cmd:"shift"}} run return run function brainfuck:compile/process/shift/
execute if data storage brainfuck:re {cur_char:{cmd:"."}} run return run function brainfuck:compile/process/output/
execute if data storage brainfuck:re {cur_char:{cmd:","}} run return run function brainfuck:compile/process/input/
execute if data storage brainfuck:re {cur_char:{cmd:"["}} run return run function brainfuck:compile/process/open_bracket/
execute if data storage brainfuck:re {cur_char:{cmd:"]"}} run return run function brainfuck:compile/process/close_bracket/
execute if data storage brainfuck:re {cur_char:{cmd:"reset"}} run return run function brainfuck:compile/process/reset/
return fail