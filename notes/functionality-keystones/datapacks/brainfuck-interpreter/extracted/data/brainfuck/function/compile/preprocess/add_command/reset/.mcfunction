data remove storage brainfuck:re code.list[-1]
data remove storage brainfuck:re code.list[-1]
data modify storage brainfuck:re code.list append value {cmd:"reset"}
scoreboard players set #addsub brainfuck.re -1
scoreboard players remove #ir_ip brainfuck.re 1
return 1