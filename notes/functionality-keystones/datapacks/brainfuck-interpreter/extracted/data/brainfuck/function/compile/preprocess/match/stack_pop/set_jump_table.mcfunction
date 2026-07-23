data modify storage brainfuck:re code.list append value {"cmd":"]","value":-1}
data modify storage brainfuck:re code.list[-1].value set from storage brainfuck:re match_pos
$data modify storage brainfuck:re code.list[$(match_pos)].value set value $(ir_ip)

#$data modify storage brainfuck:re jump_table[$(ip)] set from storage brainfuck:re match_pos
#$data modify storage brainfuck:re jump_table[$(match_pos)] set from storage brainfuck:re ip

#$tellraw @a {text:"$(ip) $(match_pos)"}