scoreboard players remove #array_length_c brainfuck.re 16
data modify storage brainfuck:re array_buffer set value [0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub, 0ub]
data modify storage brainfuck:re array append from storage brainfuck:re array_buffer[]
function brainfuck:init/array/_