# Lessons Log

- Audience: Rustory 유지보수자, LLM 에이전트
- Owner: Rustory
- Last Verified: 2026-07-12

반복 가능한 실수 방지 규칙을 누적하는 공개 로그다. 작성 규칙은 `docs/IMPROVEMENT_LOOP.md`를 따른다.

이 파일에는 일반화 가능한 제품·개발 교훈만 기록한다. 개인 배포 inventory, hostname, 실제 endpoint 주소,
정확한 fleet topology, rollout revision, checksum, 머신 로컬 경로는 이 저장소 밖의 비공개 운영 기록으로 분리한다.

## Recent Entries (max 50)

| Date | Trigger | Lesson | Applied Change | Verification |
| --- | --- | --- | --- | --- |
| 2026-07-12 | 공개 이력 감사에서 일반화되지 않은 개인 운영 증거가 source history에 포함된 사실을 확인함 | 공개 저장소의 교훈 로그는 재현 가능한 제품 규칙과 repository-owned 검증만 남기고, 실제 배포 대상·규모·주소·revision·checksum은 비공개 운영 기록으로 분리해야 한다. 현재 tree 검사만으로는 과거 tag와 Release source archive 노출을 증명할 수 없다. | 공개 이력을 정리하고 기존 tag를 재매핑하는 절차를 적용했으며 publication boundary gate에 로컬 home path, 실제 운영 endpoint·공인 IP, 교훈 로그의 상세 inventory 증거 차단을 추가했다. | repository 전체 reachable-history 검사, publication boundary self-test와 `mode=all`, secret scan, tag signature 및 Release API 검증 |
