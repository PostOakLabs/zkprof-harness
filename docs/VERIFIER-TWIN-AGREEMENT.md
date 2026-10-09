# Verifier Twin — Agreement Table

Two independent Groth16/BN254 verifiers, run over the same receipt cases. The **crate** verdict is from `zkprof-verify::verify` (risc0 3.0.5 oracle). The **twin** verdict is from `web/twin/verify.mjs` (independent JS reimplementation; JCS + sha256 + receipt-claim digest + public inputs + Groth16 pairing all derived in JS).

`agree` compares the verdict only. `digestAgree` compares the computed journal digest (a direct test that the crate and twin JCS/sha256 agree on the same journal bytes) — shown only where it is meaningful.

| case | crate | twin | agree? | journal-digest agree? | failing checks (crate) | failing checks (twin) |
|---|---|---|---|---|---|---|
| wrong-imageId/art-01 | FAIL | FAIL | YES | true | imageId expected | imageId recognized |
| mutate-journal/art-01 | FAIL | FAIL | YES | true | seal valid (Groth16 over BN254) | Groth16 pairing check |
| mutate-seal/art-01 | FAIL | FAIL | YES | true | seal valid (Groth16 over BN254) | Groth16 pairing check |
| probe-keyreorder/art-01 | PASS | PASS | YES | true | (none) | (none) |
| probe-unicode/art-01 | FAIL | FAIL | YES | true | seal valid (Groth16 over BN254) | Groth16 pairing check |
| real/art-01 | PASS | PASS | YES | true | (none) | (none) |
| real/art-106 | PASS | PASS | YES | true | (none) | (none) |
| real/rca-01 | PASS | PASS | YES | true | (none) | (none) |

## Self-check gate

The twin produced **PASS** on at least one genuine fixture (all three real fixtures, see table), so the twin verification mechanism is LIVE and its verdicts are trustworthy.

## Finding status

Every case agreed: **the adversarial pair is live.**

## Per-case checks

### wrong-imageId/art-01

- crate verdict: **FAIL**
- twin  verdict: **FAIL**
- crate journal digest: `sha256:3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de`
- twin  journal digest: `sha256:3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de`
- twin  claim digest:   `d50823d1d8c72a2f49221d2652fcee233ae193a850cfacb014ba507269272b87`

| check | twin passed | detail |
|---|---|---|
| seal decodes | true | 256 decoded bytes |
| imageId recognized | false | imageId=sha256:a1a0bc89b5b1febaeda3519f6dbade0fa5ac16beeb143c4e1b01689573567bc6 expected=sha256:0000000000000000000000000000000000000000000000000000000000000000 |
| journal present | true | object |
| claim digest derived | true | sha256:d50823d1d8c72a2f49221d2652fcee233ae193a850cfacb014ba507269272b87 oracleMatch=true |
| public inputs derived | true | [87308967599310181518572122949978443173, 114477420512449248120330787787984922587, 47763864180528442050297443023277197525, 179669847564191130769976727333722382650, 1930158958971974673407180959543112854198801264531668442085542093806106933952] oracleMatch=true |
| Groth16 pairing check | true | e(A,B)==e(α,β)·e(IC,γ)·e(C,δ) => true; sealDecode.matchOracle=true |

### mutate-journal/art-01

- crate verdict: **FAIL**
- twin  verdict: **FAIL**
- crate journal digest: `sha256:9d03055a3f2439a50f382f196dce9859164a8a16c2f58bd97bc3bc731f6f3d2f`
- twin  journal digest: `sha256:9d03055a3f2439a50f382f196dce9859164a8a16c2f58bd97bc3bc731f6f3d2f`
- twin  claim digest:   `0c5e425acd2f18518235a64127bb414804ea29d131e81683767b3b0d03256abc`

| check | twin passed | detail |
|---|---|---|
| seal decodes | true | 256 decoded bytes |
| imageId recognized | true | imageId=sha256:a1a0bc89b5b1febaeda3519f6dbade0fa5ac16beeb143c4e1b01689573567bc6 expected=(shape only) |
| journal present | true | object |
| claim digest derived | true | sha256:0c5e425acd2f18518235a64127bb414804ea29d131e81683767b3b0d03256abc oracleMatch=false |
| public inputs derived | true | [87308967599310181518572122949978443173, 114477420512449248120330787787984922587, 96045710913130387577394287544881012236, 250445997365503913277412594020466158084, 1930158958971974673407180959543112854198801264531668442085542093806106933952] oracleMatch=false |
| Groth16 pairing check | false | e(A,B)==e(α,β)·e(IC,γ)·e(C,δ) => false; sealDecode.matchOracle=true |

### mutate-seal/art-01

- crate verdict: **FAIL**
- twin  verdict: **FAIL**
- crate journal digest: `sha256:3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de`
- twin  journal digest: `sha256:3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de`
- twin  claim digest:   `d50823d1d8c72a2f49221d2652fcee233ae193a850cfacb014ba507269272b87`

| check | twin passed | detail |
|---|---|---|
| seal decodes | true | 256 decoded bytes |
| imageId recognized | true | imageId=sha256:a1a0bc89b5b1febaeda3519f6dbade0fa5ac16beeb143c4e1b01689573567bc6 expected=(shape only) |
| journal present | true | object |
| claim digest derived | true | sha256:d50823d1d8c72a2f49221d2652fcee233ae193a850cfacb014ba507269272b87 oracleMatch=true |
| public inputs derived | true | [87308967599310181518572122949978443173, 114477420512449248120330787787984922587, 47763864180528442050297443023277197525, 179669847564191130769976727333722382650, 1930158958971974673407180959543112854198801264531668442085542093806106933952] oracleMatch=true |
| Groth16 pairing check | false | pairing failed: bad point: equation left != right |

### probe-keyreorder/art-01

- crate verdict: **PASS**
- twin  verdict: **PASS**
- crate journal digest: `sha256:3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de`
- twin  journal digest: `sha256:3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de`
- twin  claim digest:   `d50823d1d8c72a2f49221d2652fcee233ae193a850cfacb014ba507269272b87`

| check | twin passed | detail |
|---|---|---|
| seal decodes | true | 256 decoded bytes |
| imageId recognized | true | imageId=sha256:a1a0bc89b5b1febaeda3519f6dbade0fa5ac16beeb143c4e1b01689573567bc6 expected=(shape only) |
| journal present | true | object |
| claim digest derived | true | sha256:d50823d1d8c72a2f49221d2652fcee233ae193a850cfacb014ba507269272b87 oracleMatch=true |
| public inputs derived | true | [87308967599310181518572122949978443173, 114477420512449248120330787787984922587, 47763864180528442050297443023277197525, 179669847564191130769976727333722382650, 1930158958971974673407180959543112854198801264531668442085542093806106933952] oracleMatch=true |
| Groth16 pairing check | true | e(A,B)==e(α,β)·e(IC,γ)·e(C,δ) => true; sealDecode.matchOracle=true |

### probe-unicode/art-01

- crate verdict: **FAIL**
- twin  verdict: **FAIL**
- crate journal digest: `sha256:179deef6d8440e6cac08cba8e872fbb98820c7ce631544577ce32817d0f3ad56`
- twin  journal digest: `sha256:179deef6d8440e6cac08cba8e872fbb98820c7ce631544577ce32817d0f3ad56`
- twin  claim digest:   `dbb1a5f480e2d37a6625c8e8bee7fa6a85f1edc3960b772c3a81ee0b737aa591`

| check | twin passed | detail |
|---|---|---|
| seal decodes | true | 256 decoded bytes |
| imageId recognized | true | imageId=sha256:a1a0bc89b5b1febaeda3519f6dbade0fa5ac16beeb143c4e1b01689573567bc6 expected=(shape only) |
| journal present | true | object |
| claim digest derived | true | sha256:dbb1a5f480e2d37a6625c8e8bee7fa6a85f1edc3960b772c3a81ee0b737aa591 oracleMatch=false |
| public inputs derived | true | [87308967599310181518572122949978443173, 114477420512449248120330787787984922587, 142200942129846594741481657270958141915, 193597271939374052485444666818179232133, 1930158958971974673407180959543112854198801264531668442085542093806106933952] oracleMatch=false |
| Groth16 pairing check | false | e(A,B)==e(α,β)·e(IC,γ)·e(C,δ) => false; sealDecode.matchOracle=true |

### real/art-01

- crate verdict: **PASS**
- twin  verdict: **PASS**
- crate journal digest: `sha256:3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de`
- twin  journal digest: `sha256:3e6136d7be3084fe01dbbe871304b2972a80b8f07bfccc37e008dbb37f4b95de`
- twin  claim digest:   `d50823d1d8c72a2f49221d2652fcee233ae193a850cfacb014ba507269272b87`

| check | twin passed | detail |
|---|---|---|
| seal decodes | true | 256 decoded bytes |
| imageId recognized | true | imageId=sha256:a1a0bc89b5b1febaeda3519f6dbade0fa5ac16beeb143c4e1b01689573567bc6 expected=(shape only) |
| journal present | true | object |
| claim digest derived | true | sha256:d50823d1d8c72a2f49221d2652fcee233ae193a850cfacb014ba507269272b87 oracleMatch=true |
| public inputs derived | true | [87308967599310181518572122949978443173, 114477420512449248120330787787984922587, 47763864180528442050297443023277197525, 179669847564191130769976727333722382650, 1930158958971974673407180959543112854198801264531668442085542093806106933952] oracleMatch=true |
| Groth16 pairing check | true | e(A,B)==e(α,β)·e(IC,γ)·e(C,δ) => true; sealDecode.matchOracle=true |

### real/art-106

- crate verdict: **PASS**
- twin  verdict: **PASS**
- crate journal digest: `sha256:592d2fb7f3231c1617bb7d8a55b2046a8045a48d9f34c09e4e1370acf834092c`
- twin  journal digest: `sha256:592d2fb7f3231c1617bb7d8a55b2046a8045a48d9f34c09e4e1370acf834092c`
- twin  claim digest:   `eda509c3a926dc7e7cd00f93f6ac65d61a3a9a53a5da876d75c3976a772d105a`

| check | twin passed | detail |
|---|---|---|
| seal decodes | true | 256 decoded bytes |
| imageId recognized | true | imageId=sha256:a1a0bc89b5b1febaeda3519f6dbade0fa5ac16beeb143c4e1b01689573567bc6 expected=(shape only) |
| journal present | true | object |
| claim digest derived | true | sha256:eda509c3a926dc7e7cd00f93f6ac65d61a3a9a53a5da876d75c3976a772d105a oracleMatch=true |
| public inputs derived | true | [87308967599310181518572122949978443173, 114477420512449248120330787787984922587, 284982721190777235868034128388848985581, 119714518539951371193474494395175287322, 1930158958971974673407180959543112854198801264531668442085542093806106933952] oracleMatch=true |
| Groth16 pairing check | true | e(A,B)==e(α,β)·e(IC,γ)·e(C,δ) => true; sealDecode.matchOracle=true |

### real/rca-01

- crate verdict: **PASS**
- twin  verdict: **PASS**
- crate journal digest: `sha256:0c0d66ea30e2e2ea55a5a80136b75b0ff4dccf97aa0a24bd5e0bb6ae3ac376bd`
- twin  journal digest: `sha256:0c0d66ea30e2e2ea55a5a80136b75b0ff4dccf97aa0a24bd5e0bb6ae3ac376bd`
- twin  claim digest:   `e760d39db2c600dfc9368e1bbc428dc46d6ebfb61c43fe17eeada3c0c6931ad4`

| check | twin passed | detail |
|---|---|---|
| seal decodes | true | 256 decoded bytes |
| imageId recognized | true | imageId=sha256:a1a0bc89b5b1febaeda3519f6dbade0fa5ac16beeb143c4e1b01689573567bc6 expected=(shape only) |
| journal present | true | object |
| claim digest derived | true | sha256:e760d39db2c600dfc9368e1bbc428dc46d6ebfb61c43fe17eeada3c0c6931ad4 oracleMatch=true |
| public inputs derived | true | [87308967599310181518572122949978443173, 114477420512449248120330787787984922587, 261262154573353336939928721817966174439, 281934332085730983349591010291325300333, 1930158958971974673407180959543112854198801264531668442085542093806106933952] oracleMatch=true |
| Groth16 pairing check | true | e(A,B)==e(α,β)·e(IC,γ)·e(C,δ) => true; sealDecode.matchOracle=true |
