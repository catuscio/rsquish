"""Sequential single-process 1/2/3/4-worker trials with exact result parity."""
import argparse,hashlib,json,re,subprocess,tempfile,time
from pathlib import Path
root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('inputs',type=Path,nargs=4,help='Four local JPEG/PNG/WebP test images; originals are never modified')
parser.add_argument('--output',type=Path,default=root/'artifacts/batch-benchmark.json')
args=parser.parse_args()
inputs=[p.resolve() for p in args.inputs]
args.output.parent.mkdir(parents=True,exist_ok=True)
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
originals={str(p):sha(p) for p in inputs}
binary=root/'target/release/rsquish'
evidence={'binarySha256':sha(binary),'originals':originals,'records':[]}
reference={}
for width in (900,2400):
 for repeat in range(2):
  for jobs in ((1,2,3,4) if repeat==0 else (4,3,2,1)):
   with tempfile.TemporaryDirectory(prefix='rasterkit-batch-') as output:
    command=['/usr/bin/time','-l',str(binary),'optimize',*map(str,inputs),'--output-dir',output,'--max-width',str(width),'--codecs','mozjpeg,webp,avif-iq,avif-ssim','--probes','7','--max-seconds','300','--jobs',str(jobs),'--quiet']
    start=time.perf_counter();run=subprocess.run(command,capture_output=True,text=True,check=True);elapsed=time.perf_counter()-start
    reports=json.loads(run.stdout); signatures=[]
    for r in reports:
     r['outputSha256']=sha(r['outputPath']);r['outputPath']=None
     signatures.append({k:r[k] for k in ('inputPath','outputSha256','score','codec','quality','outputBytes','encodeCount')} | {'candidates':[{k:c[k] for k in ('codec','quality','sha256','bytes','score','evaluationSkipped')} for c in r['candidates']]})
    reference.setdefault(width,signatures)
    assert signatures==reference[width],(width,jobs,'candidate or output mismatch')
    rss=int(re.search(r'(\d+)\s+maximum resident set size',run.stderr)[1])
    evidence['records'].append(dict(width=width,repeat=repeat,jobs=jobs,elapsedSeconds=elapsed,peakRssBytes=rss,reports=reports))
    args.output.write_text(json.dumps(evidence,indent=2))
    print(width,repeat,jobs,round(elapsed,3),'seconds',round(rss/2**20,1),'MiB exact parity',flush=True)
assert originals=={str(p):sha(p) for p in inputs}
print('PASS original files unchanged',flush=True)
