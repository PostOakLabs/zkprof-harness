# Full-Corpus Twin Agreement Sweep (corpus-sweep-8, 2026-09-02)

Two independent Groth16/BN254 receipt verifiers, run over **every** estate
receipt in `compute-proof` (316 receipt files: the four short-named
fixtures plus the 312-receipt estate corpus copied for this sweep, verbatim
estate basenames, read-only) **plus** the existing mutation/probe cases. The
**crate** verdict is from `zkprof_verify::verify` (risc0 3.0.5 oracle); the
**twin** verdict is from `web/twin/verify.mjs` (independent JS reimplementation:
JCS + sha256 + receipt-claim digest + public inputs + Groth16 pairing all derived
in JS).

- **Total cases:** 324 (316 receipts + 8 mutation/probe cases)
- **PASS|PASS:** 320 - the crate and the twin both accept.
- **FAIL|FAIL:** 4 - both reject, with the same named check.
- **Disagreement (crate != twin):** 0 - each is a FINDING reported, not resolved.
- **Not-verified on either side:** 0.
- **digestAgree:** 316/316 (0 where the two sides' computed journal digests differ); the two verifiers derived the identical journal digest across the corpus.
- **Wall time:** crate side ~ 25 s (native), twin side ~ 36.2 s (324 cases, avg 111.7 ms each).

## Findings

**No disagreement and no NOT-VERIFIED.** Every receipt in the estate corpus verifies
PASS|PASS on both the risc0 crate and the JS twin, and the 4 mutation/probe cases that are
deliberately invalid (wrong imageId, tampered journal, tampered seal, unicode probe) are
rejected FAIL|FAIL by both with the same named check. The adversarial pair is live.

## Receipt corpus

| receipt | crate | twin | agree? | digestAgree? |
|---|---|---|---|---|
| 503-canton-tokenization-readiness-diagnostic | PASS | PASS | YES | YES |
| 504-settlement-risk-capital-optimizer | PASS | PASS | YES | YES |
| 505-tokenized-collateral-eligibility-checker | PASS | PASS | YES | YES |
| 506-onchain-cash-leg-finality-checker | PASS | PASS | YES | YES |
| 507-canton-dvp-atomicity-validator | PASS | PASS | YES | YES |
| 508-repo-haircut-collateral-calculator | PASS | PASS | YES | YES |
| 509-canton-party-allowlist-validator | PASS | PASS | YES | YES |
| 510-digital-asset-regulatory-classifier | PASS | PASS | YES | YES |
| 511-multi-currency-pvp-validator | PASS | PASS | YES | YES |
| 512-tokenized-security-lifecycle-validator | PASS | PASS | YES | YES |
| 513-margin-call-collateral-mobilizer | PASS | PASS | YES | YES |
| 514-tokenized-fund-collateral-validator | PASS | PASS | YES | YES |
| 515-collateral-swap-eligibility-validator | PASS | PASS | YES | YES |
| art-01 | PASS | PASS | YES | YES |
| art-01-ap2-mandate-chain-validator | PASS | PASS | YES | YES |
| art-03-x402-settlement-modeler | PASS | PASS | YES | YES |
| art-04 | PASS | PASS | YES | YES |
| art-04-agent-identity-attestation-checker | PASS | PASS | YES | YES |
| art-05-eu-ai-act-credit-scoring-conformity | PASS | PASS | YES | YES |
| art-06-genius-act-reserve-attestation | PASS | PASS | YES | YES |
| art-09-dora-incident-classifier | PASS | PASS | YES | YES |
| art-100-mica-casp-authorization-readiness | PASS | PASS | YES | YES |
| art-101-mica-art67-own-funds-calculator | PASS | PASS | YES | YES |
| art-102-crypto-asset-whitepaper-linter | PASS | PASS | YES | YES |
| art-103-mar-crypto-surveillance-readiness | PASS | PASS | YES | YES |
| art-104-tfr-travel-rule-batch-validator | PASS | PASS | YES | YES |
| art-105-mica-token-service-scoper | PASS | PASS | YES | YES |
| art-106 | PASS | PASS | YES | YES |
| art-106-tempo-subscription-reconciler | PASS | PASS | YES | YES |
| art-107-tempo-gas-economics | PASS | PASS | YES | YES |
| art-108-canton-selective-disclosure | PASS | PASS | YES | YES |
| art-109-dtc-tokenized-treasury | PASS | PASS | YES | YES |
| art-11-vop-batch-match-rate-analyser | PASS | PASS | YES | YES |
| art-110-arc-partner-stablecoin-onboarding | PASS | PASS | YES | YES |
| art-111-arc-corridor-jurisdiction-router | PASS | PASS | YES | YES |
| art-112-dscsa-transaction-statement-verifier | PASS | PASS | YES | YES |
| art-113-saleable-returns-verifier | PASS | PASS | YES | YES |
| art-114-suspect-product-quarantine | PASS | PASS | YES | YES |
| art-115-dpp-data-carrier-validator | PASS | PASS | YES | YES |
| art-116-product-lineage-builder | PASS | PASS | YES | YES |
| art-117-product-authenticity-verifier | PASS | PASS | YES | YES |
| art-118-fsma204-cte-validator | PASS | PASS | YES | YES |
| art-119-traceability-lot-code-linker | PASS | PASS | YES | YES |
| art-12-acp-checkout-conformance-validator | PASS | PASS | YES | YES |
| art-120-recall-trace-resolver | PASS | PASS | YES | YES |
| art-121-document-integrity-anchor | PASS | PASS | YES | YES |
| art-122-timestamp-attestation-verifier | PASS | PASS | YES | YES |
| art-123-c2pa-manifest-validator | PASS | PASS | YES | YES |
| art-124-content-credential-signature-verifier | PASS | PASS | YES | YES |
| art-125-provenance-ingredient-tree-resolver | PASS | PASS | YES | YES |
| art-126-ai-act-art50-marking-checker | PASS | PASS | YES | YES |
| art-127-dual-layer-disclosure-verifier | PASS | PASS | YES | YES |
| art-128-content-binding-assertion-validator | PASS | PASS | YES | YES |
| art-129-webbotauth-signature-verifier | PASS | PASS | YES | YES |
| art-13-eudi-wallet-credential-readiness-checker | PASS | PASS | YES | YES |
| art-130-signature-directory-validator | PASS | PASS | YES | YES |
| art-131-signature-agent-card-validator | PASS | PASS | YES | YES |
| art-132-agent-key-rotation-auditor | PASS | PASS | YES | YES |
| art-133-agent-payment-rail-trust-crosswalk | PASS | PASS | YES | YES |
| art-134-agent-directory-publish-readiness | PASS | PASS | YES | YES |
| art-135-cyclonedx-sbom-validator | PASS | PASS | YES | YES |
| art-136-slsa-provenance-verifier | PASS | PASS | YES | YES |
| art-137-openvex-statement-validator | PASS | PASS | YES | YES |
| art-138-spdx-sbom-validator | PASS | PASS | YES | YES |
| art-139-cra-annex1-completeness-checker | PASS | PASS | YES | YES |
| art-14-psd3-psr-readiness-checker | PASS | PASS | YES | YES |
| art-140-cra-vuln-reporting-readiness | PASS | PASS | YES | YES |
| art-141-nis2-entity-scope-classifier | PASS | PASS | YES | YES |
| art-142-nis2-art21-gap-checker | PASS | PASS | YES | YES |
| art-143-nis2-penalty-exposure-calculator | PASS | PASS | YES | YES |
| art-144-nis2-incident-significance-scorer | PASS | PASS | YES | YES |
| art-145-nis2-ict-supply-chain-diligence-scorer | PASS | PASS | YES | YES |
| art-146-nis2-governance-readiness-checker | PASS | PASS | YES | YES |
| art-147-mcp-server-identity-attestation-validator | PASS | PASS | YES | YES |
| art-148-mcp-authorization-metadata-validator | PASS | PASS | YES | YES |
| art-149-mcp-registry-entry-conformance | PASS | PASS | YES | YES |
| art-150-mcp-tool-scope-revocation-auditor | PASS | PASS | YES | YES |
| art-151-agent-obo-mandate-validator | PASS | PASS | YES | YES |
| art-152-mcp-task-lifecycle-validator | PASS | PASS | YES | YES |
| art-153-emir-trade-report-field-validator | PASS | PASS | YES | YES |
| art-154-emir-uti-completeness-checker | PASS | PASS | YES | YES |
| art-155-emir-upi-validator | PASS | PASS | YES | YES |
| art-156-emir-counterparty-pairing-reconciler | PASS | PASS | YES | YES |
| art-157-emir-lifecycle-event-validator | PASS | PASS | YES | YES |
| art-158-emir-reporting-readiness-diagnostic | PASS | PASS | YES | YES |
| art-159-vida-einvoice-en16931-conformance-validator | PASS | PASS | YES | YES |
| art-160-vida-drr-transaction-reporter | PASS | PASS | YES | YES |
| art-161-vida-recapitulative-statement-migration-assessor | PASS | PASS | YES | YES |
| art-162-vida-platform-deemed-supplier-classifier | PASS | PASS | YES | YES |
| art-163-vida-oss-registration-router | PASS | PASS | YES | YES |
| art-164-vida-compliance-readiness-diagnostic | PASS | PASS | YES | YES |
| art-165-eudr-dds-field-validator | PASS | PASS | YES | YES |
| art-166-eudr-geolocation-plot-validator | PASS | PASS | YES | YES |
| art-167-eudr-commodity-scope-classifier | PASS | PASS | YES | YES |
| art-168-eudr-country-benchmark-risk-scorer | PASS | PASS | YES | YES |
| art-169-eudr-supply-chain-traceability-linker | PASS | PASS | YES | YES |
| art-170-eudr-readiness-diagnostic | PASS | PASS | YES | YES |
| art-171-iso42001-aims-clause-conformance | PASS | PASS | YES | YES |
| art-172-ai-risk-impact-assessment-validator | PASS | PASS | YES | YES |
| art-173-ai-system-governance-classifier | PASS | PASS | YES | YES |
| art-174-nist-ai-rmf-function-mapper | PASS | PASS | YES | YES |
| art-175-gpai-code-of-practice-conformance | PASS | PASS | YES | YES |
| art-176-ai-governance-readiness-diagnostic | PASS | PASS | YES | YES |
| art-177-ifrs17-measurement-model-classifier | PASS | PASS | YES | YES |
| art-178-ifrs17-csm-rollforward-validator | PASS | PASS | YES | YES |
| art-179-ifrs17-risk-adjustment-checker | PASS | PASS | YES | YES |
| art-180-solvency2-scr-ratio-calculator | PASS | PASS | YES | YES |
| art-181-sii-ifrs17-reconciliation-bridger | PASS | PASS | YES | YES |
| art-182-insurance-reporting-readiness-diagnostic | PASS | PASS | YES | YES |
| art-19-agentic-checkout-protocol-selector | PASS | PASS | YES | YES |
| art-195-creative-commons-license-chooser | PASS | PASS | YES | YES |
| art-196-cant-be-evil-license-selector | PASS | PASS | YES | YES |
| art-197-pil-flavor-mapper | PASS | PASS | YES | YES |
| art-198-cross-license-rights-comparator | PASS | PASS | YES | YES |
| art-199-license-election-certifier | PASS | PASS | YES | YES |
| art-20-acp-ucp-product-feed-conformance-auditor | PASS | PASS | YES | YES |
| art-200-license-election-verifier | PASS | PASS | YES | YES |
| art-202-tdmrep-reservation-builder | PASS | PASS | YES | YES |
| art-203-embedded-license-selector | PASS | PASS | YES | YES |
| art-204-license-compatibility-checker | PASS | PASS | YES | YES |
| art-205-license-terms-assembler | PASS | PASS | YES | YES |
| art-206-rights-record-builder | PASS | PASS | YES | YES |
| art-207-attribution-string-generator | PASS | PASS | YES | YES |
| art-208-royalty-split-validator | PASS | PASS | YES | YES |
| art-209-nft-metadata-validator | PASS | PASS | YES | YES |
| art-21-agent-traffic-acceptance-policy-builder | PASS | PASS | YES | YES |
| art-210-ipfs-cid-computer | PASS | PASS | YES | YES |
| art-211-prediction-market-analyzer | PASS | PASS | YES | YES |
| art-212-prediction-market-arbitrage | PASS | PASS | YES | YES |
| art-213-perp-liquidation-calculator | PASS | PASS | YES | YES |
| art-214-perp-position-lifecycle | PASS | PASS | YES | YES |
| art-215-reg-z-appendix-j-apr | PASS | PASS | YES | YES |
| art-216-trid-tolerance-cure | PASS | PASS | YES | YES |
| art-217-trid-apr-accuracy | PASS | PASS | YES | YES |
| art-218-qm-points-and-fees | PASS | PASS | YES | YES |
| art-219-qm-apr-apor-spread | PASS | PASS | YES | YES |
| art-22-agentic-payments-protocol-comparator | PASS | PASS | YES | YES |
| art-220-reg-z-threshold-lookup | PASS | PASS | YES | YES |
| art-221-llpa-stack | PASS | PASS | YES | YES |
| art-222-agency-eligibility-matrix | PASS | PASS | YES | YES |
| art-223-conforming-loan-limit | PASS | PASS | YES | YES |
| art-224-fha-mip-eligibility | PASS | PASS | YES | YES |
| art-225-va-funding-fee-residual | PASS | PASS | YES | YES |
| art-226-mismo-uldd-ulad | PASS | PASS | YES | YES |
| art-227-validate-adverse-action-notice | PASS | PASS | YES | YES |
| art-228-build-adverse-action-notice | PASS | PASS | YES | YES |
| art-229-compute-disparity-metrics | PASS | PASS | YES | YES |
| art-23-visa-trusted-agent-protocol-inspector | PASS | PASS | YES | YES |
| art-230-compute-hmda-rate-spread | PASS | PASS | YES | YES |
| art-231-compute-mla-mapr | PASS | PASS | YES | YES |
| art-232-compute-scra-rate-cap | PASS | PASS | YES | YES |
| art-233-check-card-act-ability-to-pay | PASS | PASS | YES | YES |
| art-234-test-hoepa-high-cost | PASS | PASS | YES | YES |
| art-235-test-hpml-escrow | PASS | PASS | YES | YES |
| art-236-build-ai-decision-log-record | PASS | PASS | YES | YES |
| art-237-validate-agent-audit-trail | PASS | PASS | YES | YES |
| art-238-classify-annex3-decisioning-obligations | PASS | PASS | YES | YES |
| art-239-test-bifsg-bias-thresholds | PASS | PASS | YES | YES |
| art-24-mastercard-agentic-token-builder | PASS | PASS | YES | YES |
| art-240-assess-naic-ais-program-readiness | PASS | PASS | YES | YES |
| art-241-cbpr-structured-address-linter | PASS | PASS | YES | YES |
| art-242-pacs008-party-completeness-validator | PASS | PASS | YES | YES |
| art-243-purpose-code-requirement-checker | PASS | PASS | YES | YES |
| art-244-gpi-tracker-lifecycle-simulator | PASS | PASS | YES | YES |
| art-245-mt-mx-translation-fidelity-scorer | PASS | PASS | YES | YES |
| art-246-lei-payment-binding-linter | PASS | PASS | YES | YES |
| art-247-prevalidation-readiness-scorer | PASS | PASS | YES | YES |
| art-248-compute-remittance-disclosure | PASS | PASS | YES | YES |
| art-249-compare-corridor-cost | PASS | PASS | YES | YES |
| art-25-a2a-agent-card-validator | PASS | PASS | YES | YES |
| art-250-model-stablecoin-corridor-economics | PASS | PASS | YES | YES |
| art-251-compute-parametric-trigger-payout | PASS | PASS | YES | YES |
| art-252-validate-cat-bond-trigger-terms | PASS | PASS | YES | YES |
| art-253-run-illustration-selfsupport-test | PASS | PASS | YES | YES |
| art-254-compute-rbc-action-level | PASS | PASS | YES | YES |
| art-255-compute-lcm-rate-derivation | PASS | PASS | YES | YES |
| art-256-validate-openids-homeowners-record | PASS | PASS | YES | YES |
| art-257-calculate-claims-stp-economics | PASS | PASS | YES | YES |
| art-258-parse-camt053-reconciliation | PASS | PASS | YES | YES |
| art-259-compute-multilateral-netting | PASS | PASS | YES | YES |
| art-26-x402-payload-decoder-flow-simulator | PASS | PASS | YES | YES |
| art-260-allocate-ihb-interest | PASS | PASS | YES | YES |
| art-261-test-hedge-effectiveness | PASS | PASS | YES | YES |
| art-262-validate-ebam-acmt-flow | PASS | PASS | YES | YES |
| art-263-score-cash-forecast-accuracy | PASS | PASS | YES | YES |
| art-264-validate-commission-hierarchy | PASS | PASS | YES | YES |
| art-265-amortize-asc606-commissions | PASS | PASS | YES | YES |
| art-266-reconcile-commission-statement | PASS | PASS | YES | YES |
| art-267-check-producer-license-reciprocity | PASS | PASS | YES | YES |
| art-268-compute-cdd-ownership-25pct | PASS | PASS | YES | YES |
| art-269-validate-w8-series-structural | PASS | PASS | YES | YES |
| art-27-agentic-readiness-diagnostic | PASS | PASS | YES | YES |
| art-270-perp-funding-carry | PASS | PASS | YES | YES |
| art-271-defi-lending-health | PASS | PASS | YES | YES |
| art-272-restaking-risk | PASS | PASS | YES | YES |
| art-273-pendle-yield | PASS | PASS | YES | YES |
| art-274-compile-work-mandate | PASS | PASS | YES | YES |
| art-275-genius-reserve-disclosure-checker | PASS | PASS | YES | YES |
| art-276-mutual-nda-composer | PASS | PASS | YES | YES |
| art-277-agreement-acceptance-binder | PASS | PASS | YES | YES |
| art-278-reputation-score-aggregator | PASS | PASS | YES | YES |
| art-28-mcp-server-deployability-diagnostic | PASS | PASS | YES | YES |
| art-287-revocation-status-verifier | PASS | PASS | YES | YES |
| art-29-dora-readiness-diagnostic | PASS | PASS | YES | YES |
| art-293-einvoice-format-validator | PASS | PASS | YES | YES |
| art-294-einvoice-vat-calc-verifier | PASS | PASS | YES | YES |
| art-295-einvoice-jurisdiction-mandate-router | PASS | PASS | YES | YES |
| art-296-einvoice-transmission-receipt-builder | PASS | PASS | YES | YES |
| art-298-aca-affordability-safe-harbor | PASS | PASS | YES | YES |
| art-299-aca-esrp-exposure | PASS | PASS | YES | YES |
| art-30-agent-commerce-conformance-validator | PASS | PASS | YES | YES |
| art-300-aca-226j-response-evidence-pack | PASS | PASS | YES | YES |
| art-301-section125-ndt | PASS | PASS | YES | YES |
| art-302-401k-adp-acp-test | PASS | PASS | YES | YES |
| art-303-aiuc1-control-evidence-linter | PASS | PASS | YES | YES |
| art-304-aiuc1-evidence-pack-assembler | PASS | PASS | YES | YES |
| art-305-aiuc1-evidence-freshness-lint | PASS | PASS | YES | YES |
| art-306-agent-insurability-evidence-scorer | PASS | PASS | YES | YES |
| art-307-claim-dispute-bundle-builder | PASS | PASS | YES | YES |
| art-308-pld-disclosure-pack-builder | PASS | PASS | YES | YES |
| art-309-parametric-index-deriver | PASS | PASS | YES | YES |
| art-31-a2a-x402-extension-mandate-validator | PASS | PASS | YES | YES |
| art-313-traiga-exposure-assessor | PASS | PASS | YES | YES |
| art-314-traiga-safe-harbor-pack-builder | PASS | PASS | YES | YES |
| art-315-ab2013-training-data-disclosure-linter | PASS | PASS | YES | YES |
| art-316-sb53-frontier-scope-checker | PASS | PASS | YES | YES |
| art-32-a2a-agent-card-trust-chain-validator | PASS | PASS | YES | YES |
| art-33-mcp-server-self-attestation-pack | PASS | PASS | YES | YES |
| art-34-tempo-fit-diagnostic | PASS | PASS | YES | YES |
| art-35-tempo-payments-business-case | PASS | PASS | YES | YES |
| art-355-erba-standardized-rwa-calculator | PASS | PASS | YES | YES |
| art-356-compute-oprisk-sma-2026 | PASS | PASS | YES | YES |
| art-357-basel-2023-vs-2026-capital-delta-comparator | PASS | PASS | YES | YES |
| art-358-simulate-output-floor | PASS | PASS | YES | YES |
| art-36-tempo-mpp-agent-mandate | PASS | PASS | YES | YES |
| art-364-compute-lcr-nsfr-leverage | PASS | PASS | YES | YES |
| art-369-run-rate-shock-ladder | PASS | PASS | YES | YES |
| art-37-tempo-stablecoin-issuance | PASS | PASS | YES | YES |
| art-370-supervisory-scenario-replay | PASS | PASS | YES | YES |
| art-371-simulate-var-monte-carlo | PASS | PASS | YES | YES |
| art-38-tempo-onchain-aml | PASS | PASS | YES | YES |
| art-39-tempo-zone-disclosure | PASS | PASS | YES | YES |
| art-40-tempo-agentic-checkout | PASS | PASS | YES | YES |
| art-41-tempo-validator-readiness | PASS | PASS | YES | YES |
| art-42-arc-fit-diagnostic | PASS | PASS | YES | YES |
| art-43-arc-cpn-model | PASS | PASS | YES | YES |
| art-44-arc-stablefx-model | PASS | PASS | YES | YES |
| art-45-arc-xreserve-linter | PASS | PASS | YES | YES |
| art-46-arc-paymaster-model | PASS | PASS | YES | YES |
| art-47-arc-cctp-transfer | PASS | PASS | YES | YES |
| art-48-treasury-clearing-fit-diagnostic | PASS | PASS | YES | YES |
| art-49-clearing-access-model-selector | PASS | PASS | YES | YES |
| art-50-ficc-margin-netting-estimator | PASS | PASS | YES | YES |
| art-51-cross-margining-benefit-estimator | PASS | PASS | YES | YES |
| art-52-digital-trade-fit-diagnostic | PASS | PASS | YES | YES |
| art-53-mletr-ebl-conformance-validator | PASS | PASS | YES | YES |
| art-54-digital-trade-rules-checker | PASS | PASS | YES | YES |
| art-55-trade-document-provenance-verifier | PASS | PASS | YES | YES |
| art-56-tokenized-settlement-fit-diagnostic | PASS | PASS | YES | YES |
| art-57-deposit-token-compliance-validator | PASS | PASS | YES | YES |
| art-58-cross-network-settlement-validator | PASS | PASS | YES | YES |
| art-59-settlement-asset-finality-classifier | PASS | PASS | YES | YES |
| art-60-agent-economy-runtime-fit-diagnostic | PASS | PASS | YES | YES |
| art-605-merkle-airdrop-proof-verifier | PASS | PASS | YES | YES |
| art-606-erc165-interface-id-verifier | PASS | PASS | YES | YES |
| art-608-erc2981-royalty-calculator | PASS | PASS | YES | YES |
| art-61-x402-batch-settlement-reconciler | PASS | PASS | YES | YES |
| art-610-erc4626-vault-share-math | PASS | PASS | YES | YES |
| art-611-erc7540-async-vault-request-accounting | PASS | PASS | YES | YES |
| art-62-ap2-payment-receipt-verifier | PASS | PASS | YES | YES |
| art-63-agent-service-metering-modeler | PASS | PASS | YES | YES |
| art-64-ai-act-highrisk-fit-diagnostic | PASS | PASS | YES | YES |
| art-65-ai-conformity-pack-builder | PASS | PASS | YES | YES |
| art-66-fria-postmarket-monitoring-builder | PASS | PASS | YES | YES |
| art-67-agentic-ai-risk-classifier | PASS | PASS | YES | YES |
| art-68-carbon-compliance-fit-diagnostic | PASS | PASS | YES | YES |
| art-69-cbam-embedded-emissions-calculator | PASS | PASS | YES | YES |
| art-70-cbam-default-value-resolver | PASS | PASS | YES | YES |
| art-71-cbam-certificate-cost-engine | PASS | PASS | YES | YES |
| art-72-cbam-precursor-emissions-aggregator | PASS | PASS | YES | YES |
| art-73-taxonomy-alignment-scorer | PASS | PASS | YES | YES |
| art-74-taxonomy-kpi-gar-aggregator | PASS | PASS | YES | YES |
| art-75-eugb-factsheet-validator | PASS | PASS | YES | YES |
| art-76-climate-scenario-applicator | PASS | PASS | YES | YES |
| art-77-t1-settlement-readiness-diagnostic | PASS | PASS | YES | YES |
| art-78-csdr-penalty-calculator | PASS | PASS | YES | YES |
| art-79-settlement-fail-predictor | PASS | PASS | YES | YES |
| art-80-ssi-conformance-checker | PASS | PASS | YES | YES |
| art-81-allocation-affirmation-conformance | PASS | PASS | YES | YES |
| art-82-securities-settlement-message-linter | PASS | PASS | YES | YES |
| art-83-buy-in-exposure-modeler | PASS | PASS | YES | YES |
| art-84-settlement-efficiency-kpi | PASS | PASS | YES | YES |
| art-85-pqc-timeline-fit-diagnostic | PASS | PASS | YES | YES |
| art-86-tls-pki-migration-planner | PASS | PASS | YES | YES |
| art-87-iso20022-pqc-readiness-checker | PASS | PASS | YES | YES |
| art-88-fido-pqc-conformance-checker | PASS | PASS | YES | YES |
| art-89-blockchain-quantum-risk-classifier | PASS | PASS | YES | YES |
| art-90-sanctions-screening-fit-diagnostic | PASS | PASS | YES | YES |
| art-91-ownership-50pct-aggregator | PASS | PASS | YES | YES |
| art-92-screening-list-coverage-checker | PASS | PASS | YES | YES |
| art-93-fuzzy-match-calibration-scorer | PASS | PASS | YES | YES |
| art-94-eccn-dual-use-classifier | PASS | PASS | YES | YES |
| art-95-circumvention-diligence-assessor | PASS | PASS | YES | YES |
| art-96-no-russia-clause-pack-builder | PASS | PASS | YES | YES |
| art-97-sanctions-screening-quality-scorer | PASS | PASS | YES | YES |
| art-98-mica-casp-fit-diagnostic | PASS | PASS | YES | YES |
| art-99-mica-transitional-deadline-router | PASS | PASS | YES | YES |
| cry-04-merkle-batch-verifier | PASS | PASS | YES | YES |
| cry-05-agent-action-audit-trail-aggregator | PASS | PASS | YES | YES |
| ml-01-isolation-forest | PASS | PASS | YES | YES |
| ml-03-timeseries-anomaly-detector | PASS | PASS | YES | YES |
| ptg-01-ap2-prompt-template-generator | PASS | PASS | YES | YES |
| qfa-01-options-greeks | PASS | PASS | YES | YES |
| rca-01 | PASS | PASS | YES | YES |
| rca-01-frtb-ima-pre-validator | PASS | PASS | YES | YES |
| sim-07-open-banking-consent-flow-stress | PASS | PASS | YES | YES |

## Mutation / probe cases

| case | crate | twin | agree? | digestAgree? |
|---|---|---|---|---|
| wrong-imageId/art-01 | FAIL | FAIL | YES | n/a |
| mutate-journal/art-01 | FAIL | FAIL | YES | n/a |
| mutate-seal/art-01 | FAIL | FAIL | YES | n/a |
| probe-keyreorder/art-01 | PASS | PASS | YES | n/a |
| probe-unicode/art-01 | FAIL | FAIL | YES | n/a |
| real/art-01 | PASS | PASS | YES | n/a |
| real/art-106 | PASS | PASS | YES | n/a |
| real/rca-01 | PASS | PASS | YES | n/a |

## Method

- **Crate side.** `cargo run -p zkprof-verify --bin corpus_sweep` runs `verify()` over every
  `compute-proof/*.receipt.json` and writes `fixtures/corpus-sweep/crate-verdicts.json`.
  The mutation/probe cases crate verdicts come from the existing `twin-oracle/crate-verdicts.json`.
- **Twin side.** `node web/twin/run-corpus-twin.mjs` runs the JS twin over every receipt
  (oracle per receipt from `cargo run -p zkprof-verify --bin dump`) plus the mutation/probe cases,
  and writes `fixtures/corpus-sweep/twin-verdicts.json`.
- **Provenance.** The 312 estate receipts are read-only copies (byte-identical to the estate
  originals), named by estate basename, with the batch provenance note in `PROVENANCE.md`.
  The 4 short-named fixtures are byte-identical re-copies of 4 of those estate files (their
  originals carry the estate name); so 316 files, of which 4 are duplicates by content.
  Across all 316 receipts there are **3 distinct imageIds**: 313 share the universal
  `sha256:a1a0bc89...`; 2 are `sha256:93c746e7...` (art-04, its short + estate copy); 1 is
  `sha256:d022c482...` (art-371, a genuinely distinct third image).
  Zero AINumbers-repo writes; zero estate files modified.

## Honesty register

- Verification is real on both sides: the crate verifies with risc0 3.0.5 and the twin
  reimplements the Groth16 pairing in JS; each PASS is a full seal verification, not a
  shape check.
- Wall time is a single-run measurement on one machine, not a benchmark.
- No difference was "fixed away": every disagreement and every both-FAIL receipt would be
  listed as a reported-not-resolved finding. There are none among the actual receipts.
- `digestAgree` is only shown where both sides computed a journal digest (a direct test that
  the crate and twin JCS/sha256 agree on the same journal bytes).
