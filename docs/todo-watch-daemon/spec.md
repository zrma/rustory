# `p2p-sync --watch` Daemonization Doc Spec

## 목표
- `rr p2p-sync --watch`를 “항상 켜져 있는 백그라운드 작업”으로 운영할 수 있도록, 운영체제별 서비스 설정 예시를 문서화한다.

## 범위
- macOS(launchd) 예시
- Linux(systemd --user) 예시
- 최소 환경변수/CLI 플래그로 재현 가능한 형태
- 로그 확인/재시작/중지 커맨드 포함

## 비범위
- Windows 서비스/스케줄러
- installer/패키징

