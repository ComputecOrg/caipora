#!/usr/bin/env bash
# Máquinas spot temporárias na AWS (Estocolmo, eu-north-1) para gerar dados da rede.
# Rodar no Git Bash, na raiz do repositório, com `aws login` feito.
#
#   deploy/aws_spot.sh setup
#       Prepara a região (CAIPORA_AWS_REGION): importa a chave SSH e cria o grupo caipora-ssh com
#       o IP atual. Pode repetir; também libera o IP novo se o de casa mudar.
#   deploy/aws_spot.sh start <tipo> <horas> [disco em GB, padrão 40]
#       Sobe uma máquina spot. Ela se desliga, e é apagada, sozinha depois de <horas>.
#   deploy/aws_spot.sh datagen <id> <binário linux> <rede> <prefixo> <semente> <horas>
#       Um `caipora datagen` por núcleo; para sozinho depois de <horas>.
#   deploy/aws_spot.sh fetch <id> <prefixo>
#       Traz os arquivos datagen/<prefixo>-*.txt, compactados.
#   deploy/aws_spot.sh sprt <id> <binário novo> <binário base> <tag> <concorrência> [semente]
#       SPRT de nós fixos (100 mil nós, 8moves_v3, [0, 10]) em segundo plano, numa pasta por tag.
#       Várias tags podem rodar juntas, repartindo os núcleos.
#   deploy/aws_spot.sh sprt-status <id>
#       Placar de cada SPRT (partidas, Elo, LLR, se terminou e as terminações).
#   deploy/aws_spot.sh sprt-fetch <id>
#       Traz as pastas dos SPRTs para tools/aws-sprt/.
#   deploy/aws_spot.sh stop <id>
#       Apaga a máquina na hora.
#   deploy/aws_spot.sh list
#       Máquinas do projeto que ainda existem.
#
# Custos: o orçamento "Caipora - computacao" (US$ 18,62/mês) avisa por e-mail a 50/80/100% e na
# previsão. O desligamento automático é a trava contra máquina esquecida.
set -euo pipefail
REGION=${CAIPORA_AWS_REGION:-eu-north-1}
AWS=${AWS_CLI:-"/c/Program Files/Amazon/AWSCLIV2/aws.exe"}
KEY=${CAIPORA_AWS_KEY:-$HOME/.ssh/caipora-aws.pem}
KNOWN=$HOME/.ssh/caipora-aws-known_hosts
# MSYS_NO_PATHCONV: o Git Bash converteria /aws/service/... num caminho do Windows.
aws_() { MSYS_NO_PATHCONV=1 "$AWS" --region "$REGION" "$@"; }
ssh_() {
  local ip=$1; shift
  ssh -i "$KEY" -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile="$KNOWN" \
    -o ConnectTimeout=15 "ubuntu@$ip" "$@"
}
ip_of() {
  aws_ ec2 describe-instances --instance-ids "$1" \
    --query 'Reservations[0].Instances[0].PublicIpAddress' --output text
}

cmd=${1:-}
case "$cmd" in
  setup)
    if ! aws_ ec2 describe-key-pairs --key-names caipora >/dev/null 2>&1; then
      pub=$(mktemp)
      ssh-keygen -y -f "$KEY" > "$pub"
      aws_ ec2 import-key-pair --key-name caipora \
        --public-key-material "fileb://$(cygpath -w "$pub")" --query KeyName --output text
      rm -f "$pub"
    fi
    vpc=$(aws_ ec2 describe-vpcs --filters Name=isDefault,Values=true \
      --query 'Vpcs[0].VpcId' --output text)
    sg=$(aws_ ec2 describe-security-groups --filters Name=group-name,Values=caipora-ssh \
      Name=vpc-id,Values="$vpc" --query 'SecurityGroups[0].GroupId' --output text)
    if [ "$sg" = "None" ]; then
      sg=$(aws_ ec2 create-security-group --group-name caipora-ssh --vpc-id "$vpc" \
        --description "SSH para as maquinas temporarias do Caipora" --query GroupId --output text)
    fi
    ip=$(curl -s https://checkip.amazonaws.com | tr -d '\r\n')
    aws_ ec2 authorize-security-group-ingress --group-id "$sg" --protocol tcp --port 22 \
      --cidr "$ip/32" >/dev/null 2>&1 || true
    echo "$REGION pronta: chave caipora, grupo $sg"
    ;;
  start)
    type=$2; hours=$3; disk=${4:-40}
    ami=$(aws_ ssm get-parameter \
      --name /aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id \
      --query Parameter.Value --output text)
    sg=$(aws_ ec2 describe-security-groups --filters Name=group-name,Values=caipora-ssh \
      --query 'SecurityGroups[0].GroupId' --output text)
    # A própria máquina agenda o desligamento; com "terminate", desligar = apagar.
    userdata=$(printf '#!/bin/bash\nshutdown -h +%d\n' "$((hours * 60))")
    # Spot usa sobra da AWS: sem capacidade numa zona, tenta as outras da região, da mais barata
    # para a mais cara no momento.
    zones=$(aws_ ec2 describe-spot-price-history --instance-types "$type" \
      --product-descriptions "Linux/UNIX" --start-time "$(date -u +%Y-%m-%dT%H:%M:%S)" \
      --query 'SpotPriceHistory[].[SpotPrice,AvailabilityZone]' --output text | sort -n | awk '{print $2}' | uniq)
    id=""
    for zone in $zones; do
      if id=$(aws_ ec2 run-instances --image-id "$ami" --instance-type "$type" --count 1 \
          --key-name caipora --security-group-ids "$sg" --placement "AvailabilityZone=$zone" \
          --instance-market-options \
            '{"MarketType":"spot","SpotOptions":{"SpotInstanceType":"one-time","InstanceInterruptionBehavior":"terminate"}}' \
          --instance-initiated-shutdown-behavior terminate \
          --block-device-mappings \
            "[{\"DeviceName\":\"/dev/sda1\",\"Ebs\":{\"VolumeSize\":$disk,\"VolumeType\":\"gp3\",\"DeleteOnTermination\":true}}]" \
          --user-data "$userdata" \
          --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=caipora-spot},{Key=Project,Value=caipora}]' \
          --query 'Instances[0].InstanceId' --output text 2>/dev/null); then
        echo "zona $zone" >&2
        break
      fi
      echo "sem capacidade spot em $zone" >&2
      id=""
    done
    [ -n "$id" ] || { echo "nenhuma zona de $REGION com capacidade para $type" >&2; exit 1; }
    aws_ ec2 wait instance-running --instance-ids "$id"
    ip=$(ip_of "$id")
    for _ in $(seq 1 30); do ssh_ "$ip" true 2>/dev/null && break; sleep 5; done
    echo "$id $ip ($type, apaga sozinha em ${hours} h)"
    ;;
  datagen)
    id=$2; bin=$3; net=$4; prefix=$5; seed=$6; hours=$7
    ip=$(ip_of "$id")
    ssh_ "$ip" 'mkdir -p ~/caipora/data'
    scp -q -i "$KEY" -o UserKnownHostsFile="$KNOWN" "$bin" "ubuntu@$ip:caipora/caipora"
    scp -q -i "$KEY" -o UserKnownHostsFile="$KNOWN" "$net" "ubuntu@$ip:caipora/net.nnue"
    # O datagen para no máximo 30 min antes do desligamento agendado da máquina: o desligamento
    # bloqueia logins 5 min antes, e a coleta final precisa de folga (27/09/2026: uma coleta caiu
    # nessa janela e a rodada se perdeu).
    ssh_ "$ip" "cd ~/caipora && chmod +x caipora && n=\$(nproc)
      secs=\$(( ${hours%.*} * 3600 ))
      case '$hours' in *.*) secs=\$(awk 'BEGIN { printf \"%d\", $hours * 3600 }') ;; esac
      if [ -f /run/systemd/shutdown/scheduled ]; then
        down=\$(( \$(sed -n 's/^USEC=//p' /run/systemd/shutdown/scheduled) / 1000000 ))
        limit=\$(( down - \$(date +%s) - 1800 ))
        [ \$limit -lt \$secs ] && secs=\$limit
      fi
      [ \$secs -gt 0 ] || { echo 'sem tempo antes do desligamento'; exit 1; }
      for i in \$(seq 0 \$((n - 1))); do
        s=\$(( $seed + i ))
        nohup timeout \${secs}s ./caipora datagen 100000000 data/$prefix-\$s.txt \$s 5000 net.nnue \
          > data/$prefix-\$s.log 2>&1 < /dev/null &
      done
      echo \"\$n processos de datagen, sementes $seed a \$(( $seed + n - 1 )), param em \$(( secs / 60 )) min\""
    ;;
  fetch)
    id=$2; prefix=$3
    ip=$(ip_of "$id")
    mkdir -p datagen
    out="datagen/$prefix-aws-$id.txt.gz"
    # Baixa num temporário e só troca o arquivo anterior se o novo estiver íntegro e não for menor:
    # uma coleta que falha nunca apaga a anterior (27/09/2026: apagou, e a rodada se perdeu).
    if ! ssh_ "$ip" "cd ~/caipora/data && for f in $prefix-*.txt; do head -n \$(wc -l < \$f) \$f; done | gzip -1" \
        > "$out.tmp" || ! gzip -t "$out.tmp" 2>/dev/null || [ ! -s "$out.tmp" ]; then
      rm -f "$out.tmp"
      echo "coleta falhou; mantido o arquivo anterior ($( [ -f "$out" ] && stat -c %s "$out" || echo 0 ) bytes)" >&2
      exit 1
    fi
    if [ -f "$out" ] && [ "$(stat -c %s "$out.tmp")" -lt "$(stat -c %s "$out")" ]; then
      rm -f "$out.tmp"
      echo "coleta menor que a anterior; mantida a anterior" >&2
      exit 1
    fi
    mv -f "$out.tmp" "$out"
    ls -la "$out"
    echo "posições: $(gzip -dc "$out" | wc -l)"
    ;;
  sprt)
    id=$2; new=$3; base=$4; tag=$5; conc=$6; seed=${7:-1}
    ip=$(ip_of "$id")
    ssh_ "$ip" "mkdir -p ~/caipora/bin ~/caipora/sprt/$tag"
    # Só envia o que ainda não está lá: um binário em uso por outro SPRT não pode ser sobrescrito.
    for bin in "$new" "$base"; do
      ssh_ "$ip" "test -f ~/caipora/bin/$(basename "$bin")" ||
        scp -q -i "$KEY" -o UserKnownHostsFile="$KNOWN" "$bin" "ubuntu@$ip:caipora/bin/"
    done
    ssh_ "$ip" 'test -f ~/caipora/8moves_v3.epd' ||
      scp -q -i "$KEY" -o UserKnownHostsFile="$KNOWN" tools/8moves_v3.epd "ubuntu@$ip:caipora/"
    ssh_ "$ip" "cd ~/caipora && chmod +x bin/* && if [ ! -x fastchess ]; then
        curl -sSL https://github.com/Disservin/fastchess/releases/download/v1.8.2-alpha/fastchess-linux-x86-64.tar -o fc.tar
        mkdir -p fc && tar -xf fc.tar -C fc && cp \$(find fc -type f -name fastchess | head -1) fastchess
        chmod +x fastchess
      fi
      cd sprt/$tag && nohup ../../fastchess \
        -engine cmd=../../bin/$(basename "$new") name=novo ${CAIPORA_SPRT_NEW_OPTS:-} \
        -engine cmd=../../bin/$(basename "$base") name=base \
        -each ${CAIPORA_SPRT_EACH:-tc=60+1 nodes=100000 option.Hash=16} \
        -openings file=../../8moves_v3.epd format=epd order=random -srand $seed \
        -rounds 20000 -games 2 -repeat -concurrency $conc -recover \
        -sprt elo0=0 elo1=10 alpha=0.05 beta=0.05 \
        -pgnout file=$tag.pgn -log file=$tag.log level=warn > $tag.out 2>&1 < /dev/null &
      echo \"SPRT $tag: $(basename "$new") contra $(basename "$base"), concorrência $conc\""
    ;;
  sprt-status)
    ip=$(ip_of "$2")
    ssh_ "$ip" 'cd ~/caipora/sprt && for d in */; do t=${d%/}
        games=$(grep -c "^\[Result" "$t/$t.pgn" 2>/dev/null || echo 0)
        elo=$(grep -E "^Elo:" "$t/$t.out" | tail -1 | cut -c1-40)
        llr=$(grep -E "^LLR:" "$t/$t.out" | tail -1 | cut -c1-32)
        done_=$(grep -oE "H[01] was accepted" "$t/$t.out" | tail -1)
        ends=$(grep -h "^\[Termination" "$t/$t.pgn" 2>/dev/null | sort | uniq -c | tr -s " " | tr "\n" ";")
        echo "$t | $games partidas | $elo | $llr | ${done_:-rodando} | $ends"
      done'
    ;;
  sprt-fetch)
    ip=$(ip_of "$2")
    mkdir -p tools/aws-sprt
    scp -q -r -i "$KEY" -o UserKnownHostsFile="$KNOWN" "ubuntu@$ip:caipora/sprt/*" tools/aws-sprt/
    ls tools/aws-sprt
    ;;
  stop)
    aws_ ec2 terminate-instances --instance-ids "$2" \
      --query 'TerminatingInstances[0].CurrentState.Name' --output text
    ;;
  list)
    aws_ ec2 describe-instances --filters Name=tag:Project,Values=caipora \
      Name=instance-state-name,Values=pending,running,stopping \
      --query 'Reservations[].Instances[].[InstanceId,InstanceType,State.Name,PublicIpAddress,LaunchTime]' \
      --output text
    ;;
  *)
    sed -n '2,20p' "$0"
    exit 1
    ;;
esac
