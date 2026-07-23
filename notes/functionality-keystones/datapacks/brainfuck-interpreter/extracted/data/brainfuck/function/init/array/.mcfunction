data remove storage brainfuck:re array
data remove storage brainfuck:re array_buffer
data modify storage brainfuck:re array set value []

scoreboard players operation #array_length_c brainfuck.re = #array_length brainfuck.re
function brainfuck:init/array/_

execute store result score #success brainfuck.re run data get storage brainfuck:re array
tellraw @a ["arr len: ",{score:{name:"#success",objective:"brainfuck.re"}}]