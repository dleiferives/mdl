execute if data storage brainfuck:re {code:{tmp_char:"+"}} run return run scoreboard players set #cur_char brainfuck.re 0
execute if data storage brainfuck:re {code:{tmp_char:"-"}} run return run scoreboard players set #cur_char brainfuck.re 1
execute if data storage brainfuck:re {code:{tmp_char:"<"}} run return run scoreboard players set #cur_char brainfuck.re 2
execute if data storage brainfuck:re {code:{tmp_char:">"}} run return run scoreboard players set #cur_char brainfuck.re 3
execute if data storage brainfuck:re {code:{tmp_char:","}} run return run scoreboard players set #cur_char brainfuck.re 4
execute if data storage brainfuck:re {code:{tmp_char:"."}} run return run scoreboard players set #cur_char brainfuck.re 5
execute if data storage brainfuck:re {code:{tmp_char:"["}} run return run scoreboard players set #cur_char brainfuck.re 6
execute if data storage brainfuck:re {code:{tmp_char:"]"}} run return run scoreboard players set #cur_char brainfuck.re 7
return run scoreboard players set #cur_char brainfuck.re -1