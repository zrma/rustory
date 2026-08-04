# Rustory

<p align="center">
  <img src="docs/assets/rustory-mark.svg" width="360" alt="Logotipo de Rustory: dos pergaminos unidos por un hilo rojo">
</p>

<p align="center">
  <strong>Deja huellas en local y conéctalas en P2P.</strong>
</p>

Rustory es una herramienta de historial de shell local-first que te permite registrar comandos con un solo `rr`, recuperarlos con `Ctrl-R` y sincronizarlos en múltiples dispositivos.

- **Local-first**: Cada comando se guarda primero en la base de datos SQLite del dispositivo.
- **Recuperación rápida**: Encuentra el historial que necesitas rápidamente con el familiar flujo de `Ctrl-R`.
- **Red P2P**: Conecta máquinas detrás de diferentes WiFi, NAT o routers a través de un tracker y un relay.

Este README no es un largo manual de operaciones, sino la presentación pública y el índice de referencia del proyecto. Las opciones reales, los valores predeterminados, las estructuras de datos y las ramas de ejecución se encuentran en `src/*`, `scripts/*`, `Cargo.toml`, `docs/REPO_MANIFEST.yaml` y `rr --help`.

La frontera que Rustory busca proteger es sencilla:

- Los registros se guardan primero en la base de datos SQLite local de cada dispositivo.
- El descubrimiento de la red está a cargo del tracker, mientras que la conectividad detrás de NAT es responsabilidad del relay.
- La misma red utiliza un `user_id` y una `swarm.key` compartidos, pero cada dispositivo debe tener una `identity.key` única.
- El éxito con conexión directa no es prueba de preparación para la producción. Se necesita evidencia de un circuito de relay detrás de diferentes NATs.
- El agente de IA debe poder mantener actualizados el código, la documentación y los scripts siguiendo las pruebas de aceptación.

## Inicio rápido

Instalación para conectar un nuevo dispositivo a una cuadrícula rr existente:

```sh
export RUSTORY_TRACKER_TOKEN="<fleet-token>"
curl -fsSL https://raw.githubusercontent.com/zrma/rustory/main/install/rustory.py | \
  python3 - --tracker "<https://tracker.example.com>" \
    --relay "/dns4/<relay.example.com>/tcp/4001/p2p/<relay_peer_id>" \
    --user-id "<shared-user-id>" \
    --swarm-key-b64 "<base64-swarm-key>" \
    --install-hook \
    --install-daemon \
    --import-hishtory
```

Los documentos públicos solo contienen marcadores de posición. Las URLs reales del tracker, el token, el PeerId del relay y la clave del enjambre deben guardarse en un archivo privado o en un almacén secreto, y no commitearse en este repositorio.

Después de la instalación, verifica la configuración, el tracker, el cursor del par y el estado de las filas/pendientes de eliminación con `rr doctor`, `rr sync-status --json --with-tracker` y `rr mesh --watch`. Los procedimientos detallados de incorporación, autoactualización, daemon e importación de Hishtory/Atuin se encuentran en `docs/quickstart.md`, `docs/distribution.md`, `docs/daemon.md`, `docs/hishtory-migration.md` y `docs/atuin-migration.md`.

## Navegación del agente

Cuando un agente de IA asume el control de este repositorio, el contrato de dominio se inicia en `docs/agent-harness.md`, mientras que la navegación del producto y las operaciones comienza en `docs/HANDOFF.md`. El README es una página de aterrizaje para orientarse, mientras que las reglas de ejecución reales están en los siguientes documentos:

- Reglas del agente: `AGENTS.md`
- Interfaz común del agente: `docs/agent-harness.md`
- Modelo operativo: `docs/OPERATING_MODEL.md`
- Límites de responsabilidad de la documentación: `docs/README_OPERATING_POLICY.md`
- Bucle de ejecución: `docs/EXECUTION_LOOP.md`
- Límites de salida y push: `docs/CHANGE_CONTROL.md`
- Bucle de mejora/regresión: `docs/IMPROVEMENT_LOOP.md`
- Criterios de escalada: `docs/ESCALATION_POLICY.md`
- Lecciones aprendidas: `docs/LESSONS_LOG.md`
- Ejes de mantenimiento a largo plazo: `docs/MAINTENANCE_PILLARS.md`
- Límites de confianza en seguridad y privacidad: `docs/security.md`
- Declaración de puntos de entrada y comandos de verificación: `docs/REPO_MANIFEST.yaml`

Un inicio sin contexto suele bastar con `jj status`, `find docs -maxdepth 1 -type d -name 'todo-*' | sort`, `rr --help` y `scripts/check.sh --fast`. La siguiente decisión se toma en `docs/HANDOFF.md`.

## Documentación del producto

- Incorporación rápida: `docs/quickstart.md`
- Distribución y autoactualización: `docs/distribution.md`
- P2P tracker/relay/sincronización: `docs/p2p.md`
- Daemon y gestor de servicios: `docs/daemon.md`
- Hook de shell: `docs/hook.md`
- Modelo de seguridad y privacidad: `docs/security.md`
- Migración de Hishtory: `docs/hishtory-migration.md`
- Migración de Atuin: `docs/atuin-migration.md`
- Guía de aceptación: `docs/acceptance/README.md`
- Índice completo de documentación: `docs/README.md`

## Desarrollo

El README y la documentación no reescriben la implementación, sino que apuntan a la ubicación de la fuente de verdad. Si el comportamiento cambia, primero consulta el código, la ayuda de CLI, los scripts y `docs/REPO_MANIFEST.yaml`, y luego actualiza solo la documentación necesaria.

La validación local habitual es `scripts/check.sh --fast`. Las salidas y los pushes suelen cerrarse mediante `scripts/finalize-and-push.sh --message "<type>: <summary>"`. Una aceptación más amplia, lanzamientos y mejoras de seguridad siguen las especificaciones correspondientes en `docs/todo-*` y `docs/CHANGE_CONTROL.md`.
