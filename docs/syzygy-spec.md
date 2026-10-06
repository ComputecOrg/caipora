# Especificação clean-room dos formatos Syzygy (.rtbw / .rtbz)

Documento de especificação para implementar, do zero e em Rust, um leitor de tablebases Syzygy
(WDL e DTZ) sem consultar código de terceiros. Tudo aqui é prosa e matemática: **nenhuma tabela
literal foi copiada**. Cada tabela auxiliar é dada pela sua *definição* (como calculá-la). Os
poucos números que aparecem na seção 7 são **valores de conferência** derivados das definições,
para o implementador testar o próprio código, e não para colar no código.

Escopo: xadrez padrão (um rei de cada lado), até 5 peças com garantia (o formato vai até 7; as
regras abaixo valem para 6 e 7, mas os exemplos foram conferidos só com 3 a 5 peças). Variantes
(antichess, atomic) usam modos de codificação extras que **não** são cobertos.

---

## 0. Convenções

- **Casas** são numeradas de 0 a 63: a1 = 0, b1 = 1, …, h1 = 7, a2 = 8, …, h8 = 63. Coluna
  (file) = casa mod 8 (a = 0 … h = 7); fileira (rank) = casa div 8 (fileira 1 = 0 … fileira 8 = 7).
- **Espelhamento horizontal** (troca a↔h): casa XOR 7. **Espelhamento vertical** (troca 1↔8):
  casa XOR 56. **Espelhamento diagonal** (na diagonal a1–h8, troca coluna com fileira): casa nova
  = 8·coluna + fileira.
- **off(s)** = fileira(s) − coluna(s). off = 0: casa na diagonal a1–h8; off < 0: abaixo da
  diagonal (lado de h1); off > 0: acima (lado de a8).
- **C(n, k)** é o binomial "n escolhe k", com C(n, 0) = 1 e C(n, k) = 0 quando k > n ou n < 0.
- **[condição]** vale 1 se a condição é verdadeira e 0 se é falsa.
- **LE** = little-endian, **BE** = big-endian. "u8/u16/u32/u64" = inteiro sem sinal de 8/16/32/64 bits.
- Todos os deslocamentos ("offsets") são contados **a partir do início do arquivo**. As regras de
  alinhamento (par, múltiplo de 64) são sobre esse deslocamento.
- **Código de peça** (nibble de 4 bits, usado no cabeçalho): os 3 bits baixos dão o tipo
  (1 = peão, 2 = cavalo, 3 = bispo, 4 = torre, 5 = dama, 6 = rei) e o bit de valor 8 dá a cor
  (0 = "branco da tabela", 1 = "preto da tabela"). Exemplos: 6 = rei branco, 14 (0xE) = rei preto,
  9 = peão preto.

Fontes desta seção: convenções comuns ao Stockfish `tbprobe.cpp` (derivado do código de Ronald de
Man), python-chess `chess/syzygy.py` e shakmaty-syzygy `src/table.rs`.

---

## 1. Arquivos, nomes, chave material e cabeçalho geral

### 1.1 Nome do arquivo e normalização

Um arquivo descreve uma configuração material: `<lado forte>v<lado fraco>.rtbw` (WDL) ou `.rtbz`
(DTZ). Cada lado é uma sequência de letras na ordem fixa **K, Q, R, B, N, P** (rei primeiro, peões
por último), repetidas conforme a quantidade. Ex.: `KQvK`, `KRRvK`, `KBNvK`, `KPvKP`, `KRvKN`.

Normalização (qual lado fica à esquerda, a "branca da tabela"):
1. Fica à esquerda o lado com **mais peças** (contando o rei). Ex.: `KBNvKR`, nunca `KRvKBN`;
   e `KRRvKQ`, não `KQvKRR` (contagem vence material).
2. Empate na contagem: comparar as duas strings letra a letra, com a ordem de força
   K > Q > R > B > N > P; na primeira diferença, o lado da peça mais forte fica à esquerda.
   Ex.: `KQvKR`, `KRvKN`, `KBvKN`.
3. Strings iguais: a tabela é **simétrica** (ex.: `KPvKP`, `KRvKR`).

Não existe arquivo para `KvK` (é sempre empate; o prober trata à parte).

Para achar a tabela de uma posição: monte a string W das peças brancas e B das pretas. Se "WvB"
estiver normalizado, a tabela é "WvB" e as cores da posição coincidem com as da tabela; senão a
tabela é "BvW" e a posição precisa ser **trocada de cor** (ver 3.1).

(Fontes: python-chess `normalize_tablename`, enumeração de tabelas do Stockfish `Tablebases::init`.)

### 1.2 Números mágicos

Os 4 primeiros bytes do arquivo:
- WDL (`.rtbw`): bytes 0x71, 0xE8, 0x23, 0x5D (como u32 LE: 0x5D23E871).
- DTZ (`.rtbz`): bytes 0xD7, 0x66, 0x0C, 0xA5 (como u32 LE: 0xA50C66D7).

Conferido nos arquivos reais (ver 7.1).

### 1.3 Byte 4: flags do arquivo

- bit 0 (valor 1), **split**: a tabela é **não simétrica**. Em WDL isso significa que há duas
  sub-tabelas por arquivo/coluna: uma para "branco da tabela" a jogar (lado 0) e outra para
  "preto da tabela" a jogar (lado 1). Em tabela simétrica (bit 0 = 0) só existe o lado 0.
- bit 1 (valor 2), **has pawns**: a tabela tem peões; há 4 conjuntos de sub-tabelas, um para cada
  coluna a, b, c, d do "peão líder" (ver 3.6). Sem peões, há um conjunto só.
- Observado nos arquivos reais: os 4 bits altos desse byte contêm o número total de peças
  (0x31 em `KQvK`, 0x41 em `KRvKN`, 0x33 em `KPvK`, 0x42 em `KPvKP`). Nenhum dos leitores de
  referência lê esses bits; trate como informação, não como requisito (ver Dúvidas).

**Atenção DTZ:** o bit split também vem ligado em arquivos DTZ não simétricos (ex.: `KQvK.rtbz` =
0x31), mas **DTZ sempre tem uma única sub-tabela por coluna** (um lado só, ver 5.2). Não use o
bit split para contar sub-tabelas de DTZ; use-o apenas como checagem de consistência (deve ser
igual a "tabela não simétrica").

Um leitor robusto verifica: bit 1 = "o material tem peões"; bit 0 = "o material não é simétrico".

### 1.4 Tamanho e trailer

Em todos os arquivos válidos, tamanho mod 64 = 16 (python-chess rejeita o arquivo se não for). Os
últimos 16 bytes ficam depois da última área de dados e **não são usados** pelo prober.
Verifiquei que não são o MD5 do conteúdo anterior (ver Dúvidas).

### 1.5 Descritores de peças (a partir do byte 5)

**Tabela sem peões.** No offset 5:
- 1 byte de **order**: nibble baixo = order do lado 0, nibble alto = order do lado 1.
- N bytes (N = número de peças), um por peça, em ordem: nibble baixo = código da peça na
  sequência do lado 0, nibble alto = código na sequência do lado 1.
- Depois, se o offset atual for ímpar, pula 1 byte (alinhamento par).

**Tabela com peões.** No offset 5, **sempre 4 blocos** (colunas a, b, c, d), cada um com:
- 1 byte de order (nibbles baixo/alto = lado 0/lado 1);
- se **os dois lados têm peões**, mais 1 byte de **order2** (mesma divisão de nibbles); senão, o
  order2 é tratado como 15 (ausente);
- N bytes de peças, como acima.
- Depois dos 4 blocos, alinhamento par.

**DTZ:** mesmo formato, mas só os **nibbles baixos** são usados (um lado só).

**Significado.** A sequência de peças de cada sub-tabela define (a) a ordem em que as peças da
posição são colocadas na lista de casas e (b) os grupos de codificação (ver 3.2). O order (e o
order2) define a **ordem de significância** dos grupos no índice (ver 3.3). Os dois lados de uma
mesma tabela WDL, e a WDL e a DTZ do mesmo material, **podem ter sequências diferentes**.
Exemplos reais:
- `KQvK.rtbw`: lado 0 = rei preto, rei branco, dama branca; lado 1 = rei preto, dama branca, rei branco.
- `KQvK.rtbz`: rei branco, rei preto, dama branca.
- `KRRvK.rtbw`: lado 0 = rei branco, rei preto, torre, torre; lado 1 = rei preto, rei branco, torre, torre.

Em tabelas com peões, a sequência **sempre começa pelos peões líderes**, depois (se houver) os
peões do outro lado, depois as demais peças. A cor dos peões líderes é a cor do primeiro código
da sequência do lado 0 da coluna a.

**Quem é o "branco da tabela":** é a cor com bit 8 = 0 nos descritores. Nos arquivos oficiais ele
coincide com o lado esquerdo do nome do arquivo, mas o python-chess avisa que há tabelas cuja chave
interna difere da sugerida pelo nome, e tanto ele quanto o shakmaty **derivam a chave material dos
próprios descritores**. Recomendação: calcule a chave do "branco da tabela" a partir dos códigos de
peça do lado 0 (coluna a) e valide que ela bate com o material do nome (direto ou trocado).

(Fontes: python-chess `init_table_wdl`, `setup_pieces_*`, `recalc_key`; Stockfish `set`;
shakmaty `Table::new`, `parse_pieces`; inspeção hexadecimal dos arquivos.)

### 1.6 Layout completo do arquivo

Depois dos descritores e do alinhamento par, as seções vêm nesta ordem, iterando sempre "coluna
de fora, lado de dentro" (coluna a lado 0, coluna a lado 1, coluna b lado 0, …; sem peões há só a
"coluna" 0; DTZ tem só o lado 0):

1. **Cabeçalhos de compressão** (pairs header, ver 4.1) de cada sub-tabela, um após o outro.
2. **Somente DTZ:** os **mapas DTZ** (ver 5.3) de cada coluna cujo flag tem o bit "mapped";
   depois, alinhamento par.
3. **Índices esparsos** de cada sub-tabela, em sequência (6 bytes por entrada).
4. **Tabelas de tamanho de bloco** de cada sub-tabela, em sequência (2 bytes por entrada).
5. **Dados comprimidos** de cada sub-tabela: **antes de cada uma**, alinhar o offset para o
   próximo múltiplo de 64.
6. Trailer de 16 bytes.

Sub-tabelas de valor único (single value, 4.1) ocupam 0 bytes nas seções 3, 4 e 5 (mas o
alinhamento de 64 antes da área de dados delas ainda é aplicado pelos leitores de referência; como
o tamanho é zero, isso só importa para a posição da área seguinte).

Exemplo verificado byte a byte, `KQvK.rtbw` (272 bytes):
- 0–3 magic; 4 = 0x31; 5 = order 0x00; 6–8 = peças (0xEE, 0x56, 0x65); 9 = enchimento (par).
- 10–11: pairs do lado 0: flags 0x80 (valor único), valor 4 (= vitória). Branco a jogar em KQvK é
  sempre vitória.
- 12–89: pairs do lado 1 (flags 0, bloco 2^6 = 64 bytes, span 2^15, padding 0, 2 blocos,
  comprimento de código entre 1 e 7, 7 valores lowestSym, 17 símbolos × 3 bytes) e 1 byte de
  enchimento (17 é ímpar) → termina em 90.
- 90–95: índice esparso do lado 1 (31332 posições / 32768 → 1 entrada de 6 bytes).
- 96–99: tabela de tamanhos (2 blocos + 0 de padding → 4 bytes).
- 100–127: enchimento até 128 (múltiplo de 64).
- 128–255: dados (2 blocos de 64 bytes). 256–271: trailer.

---

## 2. (Resumo) Sub-tabelas por arquivo

| Tipo | Sem peões | Com peões |
|---|---|---|
| WDL não simétrica | 2 (lado 0 e lado 1) | 8 (4 colunas × 2 lados) |
| WDL simétrica | 1 | 4 |
| DTZ (qualquer) | 1 | 4 (uma por coluna) |

O tamanho (número de índices) de cada sub-tabela é calculado a partir dos descritores (3.3), não
é gravado no arquivo. Cada sub-tabela tem seu próprio order, sequência de peças, grupos e fatores.

---

## 3. Cálculo do índice

### 3.1 Preparação: troca de cor e escolha do lado

Defina, para a posição a sondar:
- **flip** = verdadeiro se (a) a tabela é simétrica e as pretas jogam, ou (b) a tabela não é
  simétrica e o material das brancas da posição é o do "preto da tabela".
- **stm da tabela** = (pretas jogam) XOR flip. 0 → usar a sub-tabela do lado 0; 1 → lado 1.
  Em tabela simétrica isso dá sempre 0.

Com flip verdadeiro: cada peça da posição é vista com a **cor invertida** (o código de peça
procurado é o do descritor com o bit 8 invertido) e, **em tabelas com peões**, cada casa é
espelhada verticalmente (XOR 56), para que o lado tratado como "branco" ande para cima.
Em tabelas sem peões, o espelhamento vertical é dispensável (a normalização por simetria de 3.4
o absorve); python-chess não espelha, Stockfish espelha, e ambos funcionam.

### 3.2 Lista de casas, grupos e "norm"

Monte a lista de casas p[0..N−1] seguindo a sequência de peças da sub-tabela: para cada posição i
da sequência, coloque a casa de uma peça daquele tipo/cor (já com a cor ajustada pelo flip) ainda
não usada. Peças idênticas consecutivas recebem as casas em qualquer ordem (serão ordenadas depois).

**Grupos** (em ordem de cabeçalho):
- **Sem peões**, conte as "peças únicas": pares (cor, tipo) com exatamente uma peça, incluindo os
  reis. Se houver ≥ 3, o **grupo líder** são as 3 primeiras peças da sequência; se houver
  exatamente 2 (só os reis são únicos, ex.: `KRRvK`, `KNNvK`), o grupo líder são as 2 primeiras
  (que serão os dois reis). (Com 0 ou 1 peças únicas é coisa de variante; fora do escopo.)
- **Com peões**, o grupo líder são os **peões líderes**: todos os peões de uma cor, a cor do
  primeiro descritor. Peões líderes = o lado com **menos peões, desde que tenha algum**; empate →
  o "branco da tabela". Se o outro lado também tem peões, eles formam o **segundo grupo**
  ("peões restantes").
- As demais peças formam grupos de **peças idênticas consecutivas** na sequência. O gerador
  garante que peças idênticas fiquem adjacentes.

**norm[i]** = tamanho do grupo que começa na posição i (e indefinido/zero nas posições internas).
Ex.: `KRvKN` lado 0 (rei preto, torre branca, rei branco, cavalo preto) → norm = (3, –, –, 1).
`KRRvK` → (2, –, 2, –). `KPvKP` (peão branco, peão preto, rei branco, rei preto) → (1, 1, 1, 1).

### 3.3 Fatores e tamanho da sub-tabela

Cada grupo g tem uma **cardinalidade** M(g) (número de maneiras de colocá-lo) e um **fator** F(g).
O índice final é idx = Σ_g F(g) · I(g), onde I(g) ∈ [0, M(g)) é o índice local do grupo.

Cardinalidades:
- Grupo líder sem peões com 3 peças: **31332** (ver 3.5; = 6·63·62 + 4·28·62 + 4·7·28 + 4·7·6).
- Grupo líder sem peões com 2 reis: **462** (ver 3.5).
- Grupo de peões líderes com c peões, coluna f: **LeadSize(c, f)** (ver 3.6).
- Grupo de peões restantes com k peões, havendo c líderes: C(48 − c, k).
- Demais grupos com k peças: C(livres, k), onde "livres" começa em 64 − (tamanho do grupo líder)
  − (tamanho do grupo de peões restantes, se houver) e é reduzido de k **depois de cada grupo**,
  percorrendo esses grupos **na ordem do cabeçalho**.

Fatores (ordem de significância): forme a **sequência de codificação** dos grupos assim — a
posição de número `order` (0, 1, 2, …) da sequência é ocupada pelo grupo líder; a posição
`order2` (se houver peões restantes) é ocupada pelos peões restantes; as demais posições são
preenchidas pelos outros grupos **na ordem do cabeçalho**. Percorra essa sequência mantendo um
produto P, que começa em 1: para cada grupo, F(grupo) = P e depois P = P · M(grupo). Ao fim, P é o
**tamanho da sub-tabela** (número de índices).

Ou seja, o primeiro grupo da sequência de codificação é o **menos significativo** (fator 1).

Exemplos (todos conferidos, ver 7.2):
- `KQvK` (order 0): só o grupo líder, F = 1, tamanho 31332.
- `KRvKN` lado 0 (order 1): sequência = (cavalo, líder) → F(cavalo) = 1, F(líder) = 61,
  tamanho = 61 · 31332 = 1911252.
- `KRRvK` (byte 5 = 0x10). Lado 0, order 0: F(líder) = 1, F(torres) = 462, tamanho =
  462 · C(62, 2) = 873642. Lado 1, order 1: sequência = (torres, líder) → F(torres) = 1,
  F(líder) = C(62, 2) = 1891. Mesmo tamanho.
- `KPvK` coluna a, lado 0 (order 1): sequência = (rei branco, peão líder, rei preto) → F(rei
  branco) = 1, F(peão) = 63, F(rei preto) = 63 · 6 = 378; tamanho = 63 · 62 · 6 = 23436.
- `KPvKP` coluna a (peão branco líder; order = 3, order2 = 2, peças: peão branco, peão preto, rei
  branco, rei preto): sequência = (rei branco, rei preto, peões restantes, líder) → F(rei branco)
  = 1, F(rei preto) = 62, F(peão preto) = 62 · 61 = 3782, F(líder) = 3782 · C(47, 1) = 177754;
  tamanho = 177754 · 6 = 1066524.

A regra geral é sempre a do parágrafo "Fatores"; os exemplos só servem para conferir.

### 3.4 Normalização por simetria (sem peões)

Sem peões, o tabuleiro tem 8 simetrias. Aplicar, **a todas as casas da lista**, nesta ordem:
1. Se a coluna de p[0] ≥ 4 (e…h): espelhamento horizontal.
2. Se a fileira de p[0] ≥ 4 (5…8): espelhamento vertical.
   Agora p[0] está no quadrante a1–d4.
3. Procure, **entre as peças do grupo líder apenas** (as 3 ou 2 primeiras), a primeira com
   off ≠ 0. Se existir e tiver off > 0 (acima da diagonal), aplique o espelhamento diagonal a todas
   as casas. (As peças antes dela estão na diagonal, então não mudam.) Se todas as peças do grupo
   líder estiverem na diagonal, não espelhe, mesmo que peças posteriores estejam acima.

Depois disso, p[0] está no triângulo a1–d1–d4 e a primeira peça do grupo líder fora da diagonal
(se houver) está abaixo dela.

### 3.5 Grupo líder sem peões

Tabelas auxiliares (definições):
- **TRI(s)**, para s no triângulo a1–d1–d4: numere primeiro as 6 casas estritamente abaixo da
  diagonal, em ordem crescente de casa — b1, c1, d1, c2, d2, d3 → 0…5 — e depois as 4 casas da
  diagonal em ordem crescente — a1, b2, c3, d4 → 6…9.
- **LOWER(s)**, para s com off(s) < 0: numere as 28 casas abaixo da diagonal em ordem crescente
  de casa → 0…27 (b1 = 0, …, h1 = 6, c2 = 7, …, h2 = 12, d3 = 13, …, h7 = 27).
- **D(s)**, para s na diagonal a1–h8: a fileira de s (0…7).

**Caso de 3 peças únicas** (p0, p1, p2 = as três primeiras casas, já normalizadas). Sejam
a1 = [p1 > p0] e a2 = [p2 > p0] + [p2 > p1] (comparação de números de casa):
1. off(p0) ≠ 0 (p0 abaixo da diagonal, TRI ∈ 0…5):
   I = TRI(p0)·63·62 + (p1 − a1)·62 + (p2 − a2).
2. p0 na diagonal, off(p1) ≠ 0 (p1 abaixo):
   I = 6·63·62 + (D(p0)·28 + LOWER(p1))·62 + (p2 − a2).
3. p0 e p1 na diagonal, off(p2) ≠ 0 (p2 abaixo):
   I = 6·63·62 + 4·28·62 + D(p0)·7·28 + (D(p1) − a1)·28 + LOWER(p2).
4. As três na diagonal:
   I = 6·63·62 + 4·28·62 + 4·7·28 + D(p0)·7·6 + (D(p1) − a1)·6 + (D(p2) − a2).

Faixas: caso 1 ocupa [0, 23436), caso 2 [23436, 30380), caso 3 [30380, 31164), caso 4
[31164, 31332). Total 31332.

**Caso de 2 reis** (p0, p1). I = KK(TRI(p0), p1), onde a tabela KK é **construída** assim:
- contador = 0; lista "adiada" vazia.
- Para t = 0, 1, …, 9 (seja s1 a casa com TRI = t) e, para cada s2 = 0, 1, …, 63 em ordem:
  - se s2 = s1 ou s2 é vizinha de s1 (reis encostados): ilegal, pular;
  - se s1 está na diagonal e off(s2) > 0: pular (essa configuração é normalizada pelo espelho);
  - se s1 e s2 estão ambas na diagonal: anotar (t, s2) na lista adiada;
  - senão: KK(t, s2) = contador; contador += 1.
- Depois, para cada (t, s2) da lista adiada, na ordem em que foi anotada: KK(t, s2) = contador;
  contador += 1.
- O total final é 462. Conferi que esta construção reproduz exatamente a tabela usada pelos
  leitores de referência (7.3).

Combinações que não aparecem (KK indefinido) não podem ocorrer em posição legal normalizada.

### 3.6 Grupo líder com peões

Tabelas auxiliares (definições):
- **e(s)** = distância da coluna à borda = min(coluna, 7 − coluna) ∈ 0…3.
- **PT(s)** ("pawn twist"), para casas das fileiras 2 a 7: enumere as casas em pares de
  espelhamento, coluna a/h primeiro, de baixo para cima, depois b/g, c/f, d/e; dentro de cada
  par, a casa do lado a–d vem antes. Atribua 47, 46, 45, … nessa ordem. Equivalente fechado, com
  r = fileira (1…6 para fileiras 2…7):
  PT(s) = 47 − 2·(6·e(s) + r − 1) − [coluna ≥ 4].
  Ex.: a2 = 47, h2 = 46, a3 = 45, …, h7 = 36, b2 = 35, …, d7 = 1, e7 = 0.
  PT(s) é o número de casas "posteriores" disponíveis para os outros peões líderes quando o peão
  líder está em s.
- **LeadStart(c, s)** e **LeadSize(c, f)**, para c = número de peões líderes (1…5) e coluna
  f ∈ {a, b, c, d}: percorra as fileiras r = 2…7 da coluna f; LeadStart(c, casa(f, r)) = soma,
  sobre as fileiras r' < r da mesma coluna, de C(PT(casa(f, r')), c − 1); LeadSize(c, f) = a soma
  sobre todas as 6 fileiras. Para c = 1, LeadSize = 6 para qualquer coluna.

**Peão líder:** entre os peões da cor líder (casas já com o espelhamento vertical do flip), o de
**maior PT**: o mais próximo da borda e, entre os de mesma distância, o de fileira mais baixa.
(Empates de PT não ocorrem.) Coloque-o em p[0]. A coluna da sub-tabela é e(p[0]): 0 = a, 1 = b,
2 = c, 3 = d.

**Normalização:** se a coluna de p[0] ≥ 4, aplique o espelhamento horizontal a **todas** as casas.
Não há espelho vertical nem diagonal em tabelas com peões (peões têm direção).

**Índice local dos líderes:** sejam q1, …, q_{c−1} as casas dos outros peões líderes, ordenadas
por PT **crescente**. Então
I(líder) = LeadStart(c, p[0]) + Σ_{m=1}^{c−1} C(PT(q_m), m).
(É o sistema numérico combinatório sobre os PT dos peões restantes, que são todos menores que
PT(p[0]).)

**Atenção:** a sub-tabela (coluna) é escolhida **antes** de olhar o resto, e seus descritores
(sequência de peças, order, fatores) são os da coluna escolhida.

### 3.7 Demais grupos (índice combinatório)

Para cada grupo g que começa na posição i da lista (em ordem de cabeçalho), com k peças nas
posições i…i+k−1:
1. Ordene essas k casas em ordem crescente: s_1 < s_2 < … < s_k.
2. Para cada s_m, seja j_m = quantidade de casas em p[0..i−1] (todas as peças **anteriores no
   cabeçalho**, inclusive o grupo líder) menores que s_m. Seja t_m = s_m − j_m (a posição de s_m
   entre as casas ainda livres).
3. **Peões restantes** (o grupo logo após os líderes, em tabela com peões dos dois lados): use
   t_m = s_m − j_m − 8 (a fileira 1 nunca tem peão; os líderes, todos em fileiras 2–7, já estão
   descontados em j_m).
4. I(g) = Σ_{m=1}^{k} C(t_m, m).

E idx = Σ_g F(g)·I(g), com I(líder) de 3.5 ou 3.6.

### 3.8 Exemplos resolvidos (conferidos com arquivos reais)

**Exemplo A, `KQvK.rtbw`, pretas jogam:** rei preto h8, dama branca a1, rei branco e1.
- Material da posição = `KQvK` = tabela, não simétrica → flip = falso; pretas jogam → lado 1.
- Sequência do lado 1: rei preto, dama branca, rei branco → p = (63, 0, 4).
- 3 peças únicas → grupo líder de 3.
- p0 = 63 tem coluna 7 → espelho horizontal: (56, 7, 3). Fileira de 56 é 7 → espelho vertical:
  (0, 63, 59).
- Primeira do grupo líder fora da diagonal: p2 = 59 (d8, off = 4 > 0) → espelho diagonal:
  (0, 63, 31) (a1, h8, h4).
- p0, p1 na diagonal, p2 abaixo → caso 3: a1 = [63 > 0] = 1; D(a1) = 0; D(h8) = 7;
  LOWER(h4) = 21. I = 23436 + 6944 + 0 + (7 − 1)·28 + 21 = **30569**. F = 1 → idx = 30569.
- Valor descomprimido: 0 → WDL = 0 − 2 = **−2** (pretas perdem). DTZ final da posição: **−16**.

**Exemplo B, `KPvK.rtbw`, brancas jogam:** peão branco e2, rei branco e1, rei preto h1.
- Sem flip, lado 0. Único peão líder em e2 (12) → coluna e → e = 3 → sub-tabela da coluna d.
- Sequência da coluna d (peão branco, rei branco, rei preto) → p = (12, 4, 7).
- Coluna de p0 é 4 → espelho horizontal: (11, 3, 0) (d2, d1, a1).
- I(líder) = LeadStart(1, d2) = 0 (primeira fileira da coluna).
- Order da coluna d, lado 0 = 2 → sequência de codificação (rei branco, rei preto, líder) →
  F(rei branco) = 1, F(rei preto) = 63, F(líder) = 63·62 = 3906.
- Rei branco em 3: nenhuma casa anterior menor → C(3, 1) = 3. Rei preto em 0: C(0, 1) = 0.
- idx = 0·3906 + 3·1 + 0·63 = **3**. Valor 4 → WDL **+2**. DTZ = **1** (o lance de peão vencedor
  zera o contador; ver 6.2).

---

## 4. Compressão ("pairs")

O conteúdo de cada sub-tabela é a sequência de valores v[0], v[1], …, v[tamanho − 1] (um por
índice), comprimida em duas camadas: **Recursive Pairing** (Re-Pair: cada símbolo pode
representar um par de símbolos, recursivamente) e depois **código de Huffman canônico** sobre os
símbolos. Os dados são divididos em blocos de tamanho fixo; cada bloco contém um número inteiro
de símbolos.

### 4.1 Cabeçalho de compressão (por sub-tabela)

- Byte 0: **flags**. Em WDL só importa o bit 0x80. Em DTZ: bit 1 = STM, 2 = mapped, 4 = win
  plies, 8 = loss plies, 16 = wide, 128 = single value (significados em 5.2/5.3).
- **Se flags tem 0x80 (single value):** o byte 1 é o valor único (WDL: o valor bruto 0…4).
  O cabeçalho tem 2 bytes no total e a sub-tabela não tem índice esparso, tabela de tamanhos
  nem dados. Para DTZ, python-chess e shakmaty **ignoram o byte e usam 0** (baseados numa
  mensagem do autor no TalkChess); o Stockfish lê o byte. Nos arquivos que examinei o byte é 0
  (ex.: `KNvK.rtbz`), então as duas leituras coincidem.
- **Caso geral** (deslocamentos relativos ao início do cabeçalho):
  - byte 1: **blocksize** (log2 do tamanho do bloco em bytes; observados 5, 6, 9 → 32, 64, 512).
  - byte 2: **idxbits** (log2 do "span" do índice esparso).
  - byte 3: **padding** de blocos: entradas extras no fim da tabela de tamanhos.
  - bytes 4–7: **real_num_blocks**, u32 LE: número de blocos de dados.
  - byte 8: **max_len**; byte 9: **min_len** (comprimentos de código em bits, 1 ≤ min ≤ max < 64).
  - h = max_len − min_len + 1. A partir do byte 10: h valores u16 LE, **lowestSym[ℓ]** para
    ℓ = min_len, …, max_len (o primeiro símbolo de cada comprimento).
  - Em seguida: u16 LE **num_syms**.
  - Em seguida: num_syms entradas de 3 bytes (a árvore de símbolos, 4.2).
  - Se num_syms for ímpar, 1 byte de enchimento.
  - Total do cabeçalho: 12 + 2h + 3·num_syms + (num_syms mod 2) bytes.

Tamanhos das seções deste sub-tabela (usados no layout de 1.6):
- índice esparso: num_indices = ⌈tamanho / 2^idxbits⌉ entradas de 6 bytes;
- tabela de tamanhos: (real_num_blocks + padding) entradas u16 LE;
- dados: real_num_blocks · 2^blocksize bytes.

### 4.2 Árvore de símbolos (Re-Pair)

Cada símbolo s ∈ [0, num_syms) tem 3 bytes b0, b1, b2 e define dois campos de 12 bits:
- **esquerdo** = b0 + 256·(b1 AND 0x0F);
- **direito** = (b1 div 16) + 16·b2.

Se direito = 0xFFF, o símbolo é uma **folha** e representa um único valor: o próprio campo
esquerdo (em WDL basta b0; em DTZ use os 12 bits). Senão, o símbolo representa a concatenação
"tudo que o esquerdo representa" seguido de "tudo que o direito representa".

**symlen[s]** = (número de valores representados por s) − 1:
- folha: 0;
- par: symlen[esquerdo] + symlen[direito] + 1.

Calcule por recursão com memorização (ou ordem topológica). Um arquivo corrompido poderia ter
ciclo; o Stockfish detecta e rejeita, o python-chess não. Recomendo detectar.

### 4.3 Huffman canônico: tabela base

Os símbolos são codificados com Huffman canônico em que **códigos mais longos têm valor numérico
menor**. Para cada comprimento ℓ de min_len a max_len, os símbolos de comprimento ℓ são
consecutivos a partir de lowestSym[ℓ].

Construção da base (índice i = ℓ − min_len, de 0 a h − 1):
- B[h − 1] = 0;
- para i = h − 2 descendo até 0: B[i] = (B[i + 1] + lowestSym[min_len + i] − lowestSym[min_len + i + 1]) / 2
  (divisão inteira; o Stockfish rejeita o arquivo se 2·B[i] < B[i + 1]);
- **base64[i] = B[i] · 2^(64 − (min_len + i))** (o valor B alinhado à esquerda numa palavra de 64 bits).

Propriedade: base64 é não crescente em i. Um código de ℓ bits, lido como os ℓ bits mais altos de
uma janela de 64 bits W, tem comprimento ℓ exatamente quando ℓ é o **menor** comprimento com
W ≥ base64[ℓ − min_len].

Exemplo real (`KQvK.rtbw`, lado 1): min_len = 1, max_len = 7, lowestSym = (15, 15, 15, 11, 5, 2, 0)
→ B = (1, 2, 4, 4, 2, 1, 0) → base64 = (2^63, 2^63, 2^63, 2^62, 2^60, 2^58, 0). Interpretação:
qualquer janela começando com o bit 1 é o símbolo 15 (1 bit); janelas 01xx são os símbolos 11–14
(4 bits); 0001x… os símbolos 5–10 (5 bits); e assim por diante. A soma de Kraft dá exatamente 1.

### 4.4 Índice esparso e tabela de tamanhos

- **Tabela de tamanhos**: entrada b (u16 LE) = (número de valores no bloco b) − 1.
- **Índice esparso**: entrada k = 6 bytes: u32 LE **bloco** e u16 LE **offset**. Ela diz em que
  bloco e em que posição dentro do bloco está o valor de índice k·span + span/2
  (span = 2^idxbits).

### 4.5 Localizar e decodificar o valor de um índice idx

1. **Valor único:** se a sub-tabela é single value, a resposta é o valor único. Fim.
2. **Índice esparso:** k = idx div span; leia (bloco, offset) da entrada k;
   lit = offset + (idx mod span) − span/2 (pode ser negativo).
3. **Ajuste de bloco:** enquanto lit < 0: bloco = bloco − 1 e lit = lit + tamanho[bloco] + 1.
   Enquanto lit > tamanho[bloco]: lit = lit − (tamanho[bloco] + 1) e bloco = bloco + 1.
   Agora 0 ≤ lit ≤ tamanho[bloco]: o valor é o de posição lit dentro do bloco.
   (O Stockfish ainda limita bloco ao intervalo válido da tabela de tamanhos; recomendável.)
4. **Início do bloco:** endereço = início_dos_dados + bloco · 2^blocksize.
5. **Janela de bits:** W = u64 **BE** lido nos 8 primeiros bytes do bloco; o ponteiro de leitura
   avança 8 bytes; "bits consumidos" = 0.
6. **Laço de símbolos:**
   - ℓ = min_len; enquanto W < base64[ℓ − min_len]: ℓ = ℓ + 1.
   - sym = lowestSym[ℓ] + ((W − base64[ℓ − min_len]) deslocado à direita de 64 − ℓ bits).
   - Se lit < symlen[sym] + 1: o valor está dentro de sym; saia do laço.
   - Senão: lit = lit − (symlen[sym] + 1); W = W deslocado ℓ bits à esquerda (truncando em 64
     bits); bits consumidos += ℓ; se bits consumidos ≥ 32: bits consumidos −= 32 e
     W = W OR (próximo u32 **BE** do bloco deslocado à esquerda de "bits consumidos"); o ponteiro
     avança 4 bytes.
7. **Descida na árvore:** enquanto symlen[sym] > 0: seja E = esquerdo(sym). Se lit < symlen[E] + 1:
   sym = E. Senão: lit = lit − (symlen[E] + 1) e sym = direito(sym).
8. O resultado é o campo esquerdo da folha sym (WDL: 8 bits; DTZ: 12 bits).

**Bordas:** a janela pode tentar ler alguns bytes além do fim do último bloco (refill). O Stockfish
trata leitura além do fim como zeros. Na prática há sempre o trailer de 16 bytes depois da última
área de dados, mas não conte com isso: proteja a leitura.

(Fontes: Stockfish `decompress_pairs`, `set_sizes`, `set_symlen`; python-chess `setup_pairs`,
`calc_symlen`, `decompress_pairs`; shakmaty `PairsData::parse`, `read_symbols`.)

---

## 5. Significado dos valores

### 5.1 WDL

O valor bruto decodificado (0…4) menos 2, do ponto de vista de **quem joga**:

| bruto | WDL | significado |
|---|---|---|
| 0 | −2 | derrota |
| 1 | −1 | derrota "abençoada" (blessed loss): perde, mas salva pelos 50 lances |
| 2 | 0 | empate |
| 3 | +1 | vitória "amaldiçoada" (cursed win): ganha, mas não dentro dos 50 lances |
| 4 | +2 | vitória |

Os valores assumem **contador de 50 lances zerado** (posição logo após captura ou lance de peão),
**sem roque** e **sem direito de en passant**.

**Valores "don't care":** se quem joga tem uma captura vencedora, o gerador pode gravar qualquer
valor naquela posição (o que comprimir melhor); se tem uma captura que empata, a posição pode
estar gravada como derrota. Por isso o prober **sempre** precisa combinar a tabela com as
capturas (seção 6). Nunca devolva o valor cru da tabela como resposta final.

### 5.2 DTZ: lado armazenado e flags

Cada sub-tabela DTZ guarda **só um lado a jogar**: o bit 1 (STM) do flag diz qual (0 = "branco da
tabela" joga, 1 = "preto da tabela" joga), por coluna em tabelas com peões. Se o stm da tabela
(3.1) não bate com esse bit, a sub-tabela não serve e o prober faz busca de 1 lance (6.3).
Exceção: tabela **simétrica sem peões** sempre serve (o flip resolve). Simétrica **com** peões
segue a regra normal (stm da tabela é sempre 0; se o bit for 1, faça a busca).

Outros bits do flag DTZ:
- 2, **mapped**: o valor decodificado é um índice no mapa DTZ (5.3), não o valor final.
- 4, **win plies**: para WDL = +2, o valor está em **plies** exatos; sem o bit, está em lances
  (multiplicar por 2).
- 8, **loss plies**: idem para WDL = −2.
- 16, **wide**: o mapa DTZ usa entradas u16 em vez de u8.
- 128: single value (4.1).

### 5.3 Mapa DTZ

Logo após os cabeçalhos de compressão (1.6), para cada coluna (na ordem a, b, c, d; sem peões,
só uma) cujo flag tem "mapped", há **4 sub-mapas**, na ordem das classes:
índice 0 = vitória (+2), 1 = derrota (−2), 2 = vitória amaldiçoada (+1), 3 = derrota abençoada (−1).

- Sem "wide": cada sub-mapa é 1 byte n seguido de n bytes de valores.
- Com "wide": (Stockfish e python-chess alinham o offset para par antes dos 4 sub-mapas da coluna;
  o shakmaty não alinha, ver Dúvidas) cada sub-mapa é um u16 LE n seguido de n valores u16 LE.
- Depois de todas as colunas, alinhamento par.

Leitura: valor = submapa[classe do WDL][valor decodificado]. Nos mapas, os valores DTZ de cada
classe foram ordenados por frequência; o valor decodificado é o "rank" de frequência.

Exemplo real: `KQvKR.rtbz` tem flag 2 (mapped, branco a jogar, lances) e os 4 sub-mapas com 31,
2, 0 e 0 entradas.

### 5.4 Valor DTZ final

Dados o WDL w da posição (≠ 0, já resolvido pela seção 6) e o valor x da tabela (após o mapa):
1. Se (w = +2 e sem "win plies") ou (w = −2 e sem "loss plies") ou |w| = 1: x = 2·x.
2. DTZ = sinal(w) · (x + 1 + 100·[|w| = 1]).

Ou seja, a tabela guarda (distância − 1), e as classes amaldiçoada/abençoada são deslocadas em 100.
Interpretação do resultado (ponto de vista de quem joga, em plies até zerar o contador):

| DTZ | situação |
|---|---|
| 1 ≤ n ≤ 100 | vitória: um lance que zera (captura/peão, ou mate) pode ser forçado em n plies |
| n > 100 | vitória amaldiçoada (empate pela regra dos 50) |
| 0 | empate |
| −100 ≤ n ≤ −1 | derrota (−1 = vai levar mate / zerar já) |
| n < −100 | derrota abençoada |

**Arredondamento:** quando a tabela guarda lances (sem o bit de plies), o valor pode estar 1 ply
acima do real: +n pode significar vitória em n − 1 … n plies. Os geradores só usam esse
arredondamento quando ele não muda o resultado teórico. Consequência prática: preservar
DTZ + contador de 50 ≤ 99 garante a vitória; = 100 só é seguro logo após um lance que zera.

Exemplo: `KQvK.rtbz` (flag 0), brancas Db1, Re1 contra Rh8, brancas jogam: valor decodificado 6
→ 2·6 = 12 → DTZ = 13. `KRvK.rtbz`, Ra1, Th1 contra Rc3, brancas jogam: valor 13 → DTZ = 27.

(Fontes: Stockfish `map_score`, `set_dtz_map`, `probe_dtz` e comentários; python-chess
`_probe_dtz_table`, documentação de `probe_dtz`; shakmaty `probe_dtz`.)

---

## 6. Algoritmo de sondagem

Notação: "lance que zera" = captura (inclusive en passant) ou lance de peão (inclusive promoção).

### 6.0 Pré-condições

- Se há **direito de roque**, as tabelas não se aplicam: não sonde.
- Número de peças ≤ maior tabela disponível. Sondar exige **todas as tabelas alcançáveis** por
  captura e promoção, recursivamente: cada dependência sai de remover uma peça não-rei de um lado
  (captura) ou trocar um peão por N, B, R ou Q (promoção). Ex.: `KQvKR` depende de `KQvK` e
  `KRvK`; `KPvK` depende de `KQvK`, `KRvK`, `KBvK` e `KNvK`.
- `KvK` (e qualquer posição só com reis) = empate, sem arquivo.

### 6.1 WDL de uma posição (com resolução de capturas)

Função WDL(pos), de −2 a +2, do ponto de vista de quem joga:
1. melhor = −2; contagem = 0.
2. Para cada lance legal que seja **captura** (inclusive en passant):
   contagem += 1; v = −WDL(posição após o lance) (recursivo); se v > melhor, melhor = v;
   se melhor = +2, devolva +2 (marque "melhor lance zera").
3. Se contagem > 0 e **todos** os lances legais são capturas: valor = melhor (a tabela não é
   confiável aqui, porque não sabe de en passant e pode estar "don't care").
   Senão: valor = valor da tabela (seções 3–5) para a posição **ignorando** o direito de en passant.
4. Se melhor ≥ valor: devolva melhor (e, se melhor > 0 ou se o passo 3 usou "todos são capturas",
   marque "melhor lance zera"). Senão devolva valor.

Isso já cobre en passant: a captura en passant entra no passo 2 como qualquer captura, e o caso
"o único lance legal é en passant (talvez perdedor)" cai no passo 3. Se preferir o desenho do
python-chess (capturas normais na recursão e en passant só no topo), a regra equivalente no topo é:
seja v o resultado sem en passant e v1 o melhor resultado entre as capturas en passant; se
v1 ≥ v, use v1; senão, se v = 0 e todos os lances legais forem en passant, use v1.

Uma janela alfa-beta (−2, +2) pode podar o passo 2: python-chess faz isso.

### 6.2 DTZ quando a posição pode ser resolvida sem a tabela

Função DTZ(pos):
1. Rode a variante de 6.1 em que o passo 2 considera **lances que zeram** (capturas **e** lances
   de peão), com a mesma lógica (para lances de peão que não capturam, v = −WDL(posição após)).
   Obtém-se w e a marca "melhor lance zera".
2. w = 0 → DTZ = 0 (a DTZ não guarda empates).
3. Se "melhor lance zera": DTZ = +1 (w = +2), +101 (w = +1), −101 (w = −1), −1 (w = −2).
   (A tabela DTZ guarda lixo quando o melhor lance é um lance que zera vencedor ou é en passant.)
4. Senão, sonde a tabela DTZ com w (5.4). Se a sub-tabela for do lado certo, devolva o resultado.

### 6.3 DTZ quando a tabela guarda o outro lado (busca de 1 lance)

Se a sub-tabela DTZ guarda o outro lado a jogar:
- Para cada lance legal: se ele zera, d = −(DTZ "antes de zerar" do WDL da posição filha:
  +1/+101/0/−101/−1 conforme WDL(filha) = 2/1/0/−1/−2); senão, d = −DTZ(filha), ajustado em 1 ply
  para longe do zero (d > 0 → d + 1; d < 0 → d − 1). Se o lance dá mate, considere d = 1.
- Fique com o **menor** d que tenha o **mesmo sinal** de w (para vitória: o mais curto; para
  derrota: o mais negativo, isto é, a resistência mais longa).
- Sem lances legais: −1 (mate).

Python-chess faz a mesma coisa de forma ligeiramente diferente (no lado que ganha, só lances que
não zeram, pois os que zeram já foram tratados em 6.2; no lado que perde, todos). O resultado é o
mesmo.

### 6.4 Escolha de lance na raiz (com contador de 50 lances)

A DTZ acima supõe contador zerado. Na raiz, com contador c (meios-lances desde o último lance que
zera):
- Para cada lance legal m: se m zera, v = (−WDL(filha)) convertido em +1/+101/0/−101/−1; senão
  v = −DTZ(filha) ajustado em 1 ply para longe do zero. Lance que dá mate: v = 1.
- **WDL efetivo com a regra dos 50:** v > 0 é vitória real se v + c ≤ 100 (senão amaldiçoada);
  v < 0 é derrota real se −v + c ≤ 100.
- Política usada por Stockfish/Fathom (uma possibilidade, não faz parte do formato): rank =
  1000 se 0 < v e v + c ≤ 99 (e não houve repetição desde o último lance que zera), senão
  1000 − (v + c) para v > 0; −1000 se v < 0 e −2v + c < 100, senão −1000 + (−v + c); 0 para v = 0.
  Jogue lances de rank máximo; entre vitórias seguras, a busca normal escolhe.
- Sem DTZ disponível: use WDL de cada filha (com a mesma noção de amaldiçoada/abençoada).

Na **busca** (não na raiz), o uso usual é: sondar WDL só com contador zerado (logo após lance que
zera) e sem roque; o valor é exato nesse caso.

(Fontes: Stockfish `search`, `probe_wdl`, `probe_dtz`, `root_probe`; python-chess `probe_ab`,
`probe_wdl`, `probe_dtz_no_ep`, `probe_dtz`; Fathom `root_probe_dtz`, `dtz_to_wdl`.)

---

## 7. Fatos de verificação

Todos os itens abaixo foram conferidos por mim com os arquivos oficiais de 3 a 5 peças
(tablebase.lichess.ovh) e o python-chess 1.11.2 como oráculo. São para **testes**, não para
virarem constantes no código: o código deve calculá-los pelas definições.

### 7.1 Bytes de cabeçalho

- `KQvK.rtbw`: começa com 71 E8 23 5D, byte 4 = 0x31, byte 5 = 0x00, bytes 6–8 = EE 56 65,
  bytes 10–11 = 80 04 (lado 0 single value 4); tamanho 272.
- `KQvK.rtbz`: D7 66 0C A5, 0x31, 0x00, bytes 6–8 = 06 0E 05; flag do pairs = 0.
- `KNvK.rtbw` e `KBvK.rtbw`: 80 bytes, os dois lados single value 2 (empate). `KNvK.rtbz`:
  single value com byte 0.
- `KPvK.rtbw`: byte 4 = 0x33; colunas a e b com order 0x21, colunas c e d com 0x22.
- `KPvKP.rtbw`: byte 4 = 0x42 (simétrica, com peões); cada coluna tem 2 bytes de order.
- Todos os arquivos: tamanho mod 64 = 16.

### 7.2 Tamanhos de sub-tabela (número de índices)

- `KQvK`: 31332 por lado. `KRvKN`: 1911252. `KRRvK`: 873642. `KPvK`: 23436 por coluna e lado.
  `KPvKP`: 1066524 por coluna.
- LeadSize(c, coluna a…d): c = 1 → 6, 6, 6, 6; c = 2 → 252, 180, 108, 36;
  c = 3 → 5201, 2645, 953, 125; c = 4 → 70315, 25375, 5491, 295.
- KK: 462 entradas; com TRI = 0 (rei em b1), as casas a1, b1, c1, a2, b2, c2 são inválidas e d1
  recebe o código 0.

### 7.3 Funções auxiliares

- TRI: b1, c1, d1, c2, d2, d3, a1, b2, c3, d4 → 0…9.
- LOWER(h4) = 21; LOWER(b1) = 0; LOWER(h7) = 27.
- PT(a2) = 47, PT(h2) = 46, PT(b2) = 35, PT(d7) = 1, PT(e7) = 0.
- A tabela KK construída por 3.5 coincidiu entrada por entrada com a dos leitores de referência;
  a fórmula fechada de PT coincidiu casa por casa.

### 7.4 Posições (WDL, DTZ do ponto de vista de quem joga)

| FEN | idx WDL / bruto | WDL | DTZ |
|---|---|---|---|
| 7k/8/8/8/8/8/8/Q3K3 b - - 0 1 | 30569 / 0 | −2 | −16 |
| 7k/8/8/8/8/8/8/1Q2K3 w - - 0 1 | 24791 / 4 (DTZ: idx 11227, valor 6) | +2 | 13 |
| 8/8/8/8/8/2k5/8/K6R w - - 0 1 | 30778 / 4 (DTZ: idx 30414, valor 13) | +2 | 27 |
| 8/8/8/8/8/2k5/8/K6R b - - 0 1 | (DTZ guarda brancas: busca de 1 lance) | −2 | −30 |
| 8/8/8/8/8/8/4P3/4K2k w - - 0 1 | 3 / 4 | +2 | 1 |
| 8/2K5/4B3/3N4/8/8/4k3/8 b - - 0 1 | | −2 | −53 |
| 8/8/8/8/8/8/8/KQ5k b - - 0 1 | | −2 | −16 |
| k7/8/1K6/8/8/8/8/2Q5 b - - 0 1 | | −2 | −4 |
| 1k6/1P6/1K6/8/8/8/8/8 b - - 0 1 (afogado) | | 0 | 0 |

### 7.5 Fatos gerais (testes de propriedade)

- `KNvK`, `KBvK`, `KvK`: toda posição legal é empate (as duas primeiras são single value 2).
- `KQvK` com o lado da dama a jogar: toda posição legal é vitória (o lado 0 é single value 4).
  Com o rei sozinho a jogar: derrota, exceto afogamento ou captura da dama desprotegida (empate).
- `KRvK` com a torre a jogar: sempre vitória.
- Uma posição e sua imagem por qualquer simetria válida (8 sem peões; espelho horizontal com
  peões) e pela troca de cores (com espelho vertical e troca de quem joga) dão o mesmo resultado.
  Ótimo teste automatizado: sorteie posições, aplique simetrias, compare.
- Consistência DTZ: em vitória com DTZ = n > 1 sem lance que zera, existe lance para uma filha com
  DTZ = −(n − 1) (ou −(n − 2) quando a tabela arredonda).
- Consistência WDL × busca: WDL(pos) = max sobre lances legais de −WDL(filha) em posições onde
  todos os lances levam a tabelas disponíveis (cuidado com a regra dos 50 nas classes ±1).

### 7.6 Armadilhas comuns

1. **Esquecer as capturas** e devolver o valor cru da tabela: dá resultado errado em posições
   "don't care" (5.1).
2. **Usar o split para contar sub-tabelas DTZ**: a DTZ tem sempre uma por coluna.
3. **Supor a mesma ordem de peças em WDL e DTZ**, ou nos dois lados da WDL: cada sub-tabela tem a
   sua (1.5).
4. **Byte order misturado**: tudo do cabeçalho é LE, mas a janela de bits dos blocos é **BE**
   (u64 inicial e u32 de recarga).
5. **Alinhamentos**: par depois dos descritores, par depois de cada árvore de símbolos com
   num_syms ímpar, par depois dos mapas DTZ, 64 antes de **cada** área de dados.
6. **Espelho diagonal decidido fora do grupo líder**: só as 3 (ou 2) primeiras peças contam.
7. **Contar j (casas anteriores menores) com a ordem de codificação** em vez da ordem do
   cabeçalho: j usa todas as peças **anteriores no cabeçalho**.
8. **Esquecer o "−8"** dos peões restantes, ou aplicá-lo a outros grupos.
9. **Escolher o peão líder pelo número de casa** em vez do maior PT (borda primeiro, depois
   fileira mais baixa).
10. **Esquecer o espelho vertical** das casas quando há troca de cor em tabela com peões.
11. **Multiplicar por 2 errado na DTZ**: as classes ±1 são sempre multiplicadas; ±2 só sem o bit
    de plies correspondente.
12. **Confiar na DTZ quando o melhor lance zera** (captura/peão vencedor ou en passant): a tabela
    guarda lixo; use 6.2.
13. **Ignorar o contador de 50 lances na raiz**: uma "vitória" com DTZ + contador > 100 é empate.
14. **Sondar com direito de roque**.
15. **Overflow**: índices de 6–7 peças passam de 2^32; use u64 para idx, fatores e tamanhos.
    A janela de bits precisa de deslocamento lógico em 64 bits com truncamento.
16. **Leitura além do fim** do último bloco na recarga da janela (4.5).

---

## 8. Fontes por seção

| Seção | Fontes |
|---|---|
| 0, 1 | Stockfish `src/syzygy/tbprobe.cpp` (derivado do tbprobe de R. de Man), python-chess `chess/syzygy.py`, shakmaty-syzygy `src/table.rs`; inspeção hexadecimal dos arquivos oficiais |
| 2, 3 | python-chess (`set_norm_*`, `calc_factors_*`, `encode_piece`, `encode_pawn`), Stockfish (`set_groups`, `do_probe_table`, `Tablebases::init`), shakmaty (`group_pieces`, `GroupData::new`, `encode`); exemplos conferidos com python-chess como oráculo |
| 4 | Stockfish (`set_sizes`, `set_symlen`, `decompress_pairs` e comentários sobre Re-Pair/Huffman canônico), python-chess (`setup_pairs`, `decompress_pairs`), shakmaty (`PairsData::parse`) |
| 5 | Stockfish (`TBFlag`, `map_score`, `set_dtz_map`, comentários de `probe_dtz`), python-chess (documentação de `probe_dtz`), shakmaty (`Flag`, `probe_dtz`) |
| 6 | Stockfish (`search`, `probe_wdl`, `probe_dtz`, `root_probe`), python-chess (`probe_ab`, `probe_wdl`, `probe_dtz_no_ep`, `probe_dtz`), Fathom `src/tbprobe.c` (`root_probe_dtz`, `dtz_to_wdl`) |
| 7 | Execução própria: arquivos de tablebase.lichess.ovh/tables/standard (3-4-5-wdl e 3-4-5-dtz) + python-chess 1.11.2 |

Não consegui baixar diretamente o `tbprobe.c`/`tbcore.c` do repositório syzygy1/tb (o caminho
tentado devolveu vazio) nem li a página "Syzygy Bases" da Chess Programming Wiki; o conteúdo
equivalente veio dos derivados acima, que reproduzem a lógica original.

---

## 9. Dúvidas e pontos não confirmados

1. **Bits altos do byte 4**: observei que contêm o número de peças, mas nenhum leitor os usa.
   Não confirmado como parte do formato.
2. **Trailer de 16 bytes**: não é o MD5 do conteúdo anterior (testei). Talvez seja um checksum de
   outro tipo usado pelas ferramentas de verificação. Os probers ignoram.
3. **Valor single value em DTZ**: python-chess/shakmaty usam 0; Stockfish lê o byte. Nos arquivos
   vistos o byte é 0. Recomendo usar 0 e, em modo de depuração, verificar que o byte é 0.
4. **Alinhamento antes de mapas DTZ "wide"**: Stockfish e python-chess (este só no caso com peões)
   alinham para par antes; shakmaty não. Só importa em tabelas com mapa wide (endgames muito
   longos, em geral de 7 peças); para ≤ 5 peças provavelmente não aparece. Não verificado num
   arquivo real com mapa wide.
5. **Chave interna diferente do nome do arquivo**: python-chess cita casos em que o material
   indicado pelo nome difere da chave interna. Não identifiquei quais tabelas são; daí a
   recomendação de derivar a chave dos descritores (1.5).
6. **"Padding" de blocos** (byte 3 do pairs): entendo que sejam entradas extras na tabela de
   tamanhos para o índice esparso não apontar para fora; o valor delas não é necessário para a
   decodificação correta. Não testei um arquivo com padding ≠ 0.
7. **Política de rank na raiz** (6.4): é escolha de engine (Stockfish/Fathom), não do formato.
   Os limiares 99/100 e o "−2v + c < 100" para derrotas vêm do Fathom e não os conferi contra o
   texto original do autor.
8. **Tabelas com 6–7 peças**: as regras valem em tese (até 5 peões líderes, mapas wide), mas
   todos os exemplos e conferências foram feitos com 3–5 peças.
