/**
 * The room, seen from the outside: where the speakers point, where each light sits, and a handle to drag it somewhere else.
 *
 * Everything drawn here uses the engine's own coordinate convention — listener at the origin, metres, +X right, +Y up, +Z toward the front speakers —
 * so what the view shows and what `spatial.rs` weights against are the same numbers.
 */

import { Grid, Html, OrbitControls, TransformControls } from "@react-three/drei";
import { Canvas } from "@react-three/fiber";
import { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as THREE from "three";

import type { DeviceEntry, RoomBounds, SpeakerPlacement } from "../bindings";
import {
  add,
  centroid,
  clampToRoom,
  deviceExtent,
  deviceLabel,
  devicePath,
  devicePosition,
  isStrip,
  len,
  normalize,
  roomCenter,
  rotateLeg,
  roomSize,
  round,
  scale,
  speakerRadius,
  sub,
  type Vec3,
} from "../geometry";
import { useThemeColors } from "../hooks";
import { replaceAt } from "../util";

export type TransformMode = "translate" | "rotate";

/** What of a bent strip is picked: one of its corners, to drag, or one of its legs, to turn. */
type PathPart = { kind: "corner" | "leg"; at: number };

const partKey = (index: number, part: PathPart) => `${index}:${part.kind === "corner" ? "c" : "l"}${part.at}`;

const X_AXIS = new THREE.Vector3(1, 0, 0);
const Y_AXIS = new THREE.Vector3(0, 1, 0);
const STRIP_RADIUS = 0.055;

export function RoomView({
  room,
  speakers,
  devices,
  selected,
  mode,
  onSelect,
  onMove,
  onOrient,
  onPath,
}: {
  room: RoomBounds;
  speakers: SpeakerPlacement[];
  devices: DeviceEntry[];
  /** Index into `devices`, or null. Only a positioned device can be selected. */
  selected: number | null;
  mode: TransformMode;
  onSelect: (index: number | null) => void;
  onMove: (index: number, position: Vec3) => void;
  /** Called while rotating a strip; the length is preserved, only the direction changes. */
  onOrient: (index: number, direction: Vec3) => void;
  /** Called while dragging a bent strip: a single corner, or every corner at once when the whole strip is moved. */
  onPath: (index: number, points: Vec3[]) => void;
}) {
  const colors = useThemeColors({
    accent: "--accent",
    text: "--text",
    muted: "--text-muted",
    surface: "--surface",
    border: "--border",
    strong: "--border-strong",
    bg: "--bg-deep",
  });

  const center = roomCenter(room);
  const size = roomSize(room);
  // Framed off the room's longest side rather than per-axis multiples, so a long thin room does not start out as a speck in the corner.
  const span = Math.max(size[0], size[1], size[2], 1);
  const start: Vec3 = [center[0] + span * 0.5, center[1] + span * 0.42, center[2] + span * 0.92];

  return (
    <div className="roomview">
      <Canvas camera={{ position: start, fov: 45, near: 0.05, far: 200 }} dpr={[1, 2]} style={{ background: colors.bg }}>
        <Suspense fallback={null}>
          <ambientLight intensity={1.6} />
          <directionalLight position={[4, 8, 6]} intensity={1.1} />

          <Scene
            room={room}
            speakers={speakers}
            devices={devices}
            selected={selected}
            mode={mode}
            colors={colors}
            onSelect={onSelect}
            onMove={onMove}
            onOrient={onOrient}
            onPath={onPath}
          />

          <OrbitControls makeDefault target={center} enableDamping dampingFactor={0.12} minDistance={1} maxDistance={60} />
        </Suspense>
      </Canvas>
    </div>
  );
}

type Colors = Record<"accent" | "text" | "muted" | "surface" | "border" | "strong" | "bg", string>;

function Scene({
  room,
  speakers,
  devices,
  selected,
  mode,
  colors,
  onSelect,
  onMove,
  onOrient,
  onPath,
}: {
  room: RoomBounds;
  speakers: SpeakerPlacement[];
  devices: DeviceEntry[];
  selected: number | null;
  mode: TransformMode;
  colors: Colors;
  onSelect: (index: number | null) => void;
  onMove: (index: number, position: Vec3) => void;
  onOrient: (index: number, direction: Vec3) => void;
  onPath: (index: number, points: Vec3[]) => void;
}) {
  const center = roomCenter(room);
  const size = roomSize(room);
  const radius = speakerRadius(room);

  // Callback refs rather than a ref array: TransformControls needs the live object of whichever device is selected.
  const objects = useRef(new Map<number, THREE.Object3D>());
  // Keyed `device:c<corner>` and `device:l<leg>`: a bent strip is not one rigid thing, it is dragged corner by corner and turned leg by leg.
  const parts = useRef(new Map<string, THREE.Object3D>());
  const [handle, setHandle] = useState<THREE.Object3D | null>(null);
  // What carries the gizmo, and on which device — picking a part selects its device in the same click, so the two have to arrive together.
  const [picked, setPicked] = useState<{ device: number; part: PathPart } | null>(null);
  const part = picked !== null && picked.device === selected ? picked.part : null;

  const register = useCallback((index: number) => {
    return (object: THREE.Object3D | null) => {
      if (object) objects.current.set(index, object);
      else objects.current.delete(index);
    };
  }, []);

  const registerPart = useCallback((index: number, part: PathPart) => {
    const key = partKey(index, part);
    return (object: THREE.Object3D | null) => {
      if (object) parts.current.set(key, object);
      else parts.current.delete(key);
    };
  }, []);

  // A leg is only grabbed while turning: dragging one around would be a move of the whole strip, which the centre handle already is.
  const grabbing: PathPart | null = part === null ? null : part.kind === "corner" || mode === "rotate" ? part : null;

  useEffect(() => {
    if (selected === null) {
      setHandle(null);
      return;
    }
    const grabbed = grabbing === null ? objects.current.get(selected) : parts.current.get(partKey(selected, grabbing));
    setHandle(grabbed ?? null);
  }, [selected, grabbing?.kind, grabbing?.at, devices]);

  const selectedDevice = selected === null ? undefined : devices[selected];
  const selectedPath = selectedDevice === undefined ? null : devicePath(selectedDevice);
  // A bent strip has one direction per leg rather than one overall, so turning it means turning the leg that is picked.
  const canRotate =
    selectedDevice !== undefined &&
    (selectedPath !== null ? grabbing?.kind === "leg" : isStrip(selectedDevice) && len(deviceExtent(selectedDevice)) > 1e-3);

  const handleChange = () => {
    if (selected === null || !handle) return;
    const world = handle.getWorldPosition(new THREE.Vector3());
    const dragged = round([world.x, world.y, world.z]);

    if (selectedPath) {
      if (grabbing?.kind === "corner") {
        onPath(selected, replaceAt(selectedPath, grabbing.at, clampToRoom(dragged, room)));
      } else if (grabbing?.kind === "leg") {
        // The gizmo sits on the leg's first corner and its local +X runs along the leg, so the turned axis is the leg's new direction.
        const direction = X_AXIS.clone().applyQuaternion(handle.quaternion);
        onPath(selected, rotateLeg(selectedPath, grabbing.at, round([direction.x, direction.y, direction.z], 4)));
      } else {
        // The whole strip follows its centre handle, so the shape is kept and only the offset changes.
        const delta = sub(clampToRoom(dragged, room), centroid(selectedPath));
        onPath(selected, selectedPath.map((point) => round(add(point, delta))));
      }
      return;
    }

    if (mode === "translate") {
      onMove(selected, clampToRoom(dragged, room));
    } else {
      const direction = X_AXIS.clone().applyQuaternion(handle.quaternion);
      onOrient(selected, round([direction.x, direction.y, direction.z], 4));
    }
  };

  return (
    <>
      {/* A click that reaches the floor rather than an object means "nothing selected". */}
      <mesh
        position={[center[0], Math.min(room.min[1], room.max[1]), center[2]]}
        rotation={[-Math.PI / 2, 0, 0]}
        onPointerDown={() => onSelect(null)}
      >
        <planeGeometry args={[size[0], size[2]]} />
        <meshStandardMaterial color={colors.surface} transparent opacity={0.35} />
      </mesh>

      <Grid
        position={[center[0], Math.min(room.min[1], room.max[1]) + 0.002, center[2]]}
        args={[size[0], size[2]]}
        cellSize={0.5}
        cellColor={colors.border}
        sectionSize={1}
        sectionColor={colors.strong}
        fadeDistance={Math.max(size[0], size[2]) * 3}
        fadeStrength={1}
        infiniteGrid={false}
      />

      <RoomBox room={room} color={colors.muted} />
      <Listener color={colors.muted} />

      {speakers.map((speaker) => (
        <Speaker key={speaker.channel} placement={speaker} radius={radius} colors={colors} />
      ))}

      {devices.map((device, index) =>
        devicePosition(device) === null && devicePath(device) === null ? null : (
          <DeviceObject
            key={index}
            device={device}
            index={index}
            active={index === selected}
            activePart={index === selected ? part : null}
            colors={colors}
            onSelect={(device) => {
              setPicked(null);
              onSelect(device);
            }}
            onSelectPart={(device, picked) => {
              setPicked({ device, part: picked });
              onSelect(device);
            }}
            register={register(index)}
            registerPart={registerPart}
          />
        ),
      )}

      {handle && (
        <TransformControls
          object={handle}
          mode={mode === "rotate" && canRotate ? "rotate" : "translate"}
          size={0.7}
          space="world"
          onObjectChange={handleChange}
        />
      )}
    </>
  );
}

function RoomBox({ room, color }: { room: RoomBounds; color: string }) {
  const geometry = useMemo(() => {
    const size = roomSize(room);
    return new THREE.EdgesGeometry(new THREE.BoxGeometry(Math.max(size[0], 0.01), Math.max(size[1], 0.01), Math.max(size[2], 0.01)));
  }, [room.min[0], room.min[1], room.min[2], room.max[0], room.max[1], room.max[2]]);

  const center = roomCenter(room);
  return (
    <lineSegments position={center} geometry={geometry}>
      <lineBasicMaterial color={color} />
    </lineSegments>
  );
}

/** The listening position, which is the origin every other coordinate is measured from. */
function Listener({ color }: { color: string }) {
  const { t } = useTranslation();
  return (
    <group>
      <mesh>
        <sphereGeometry args={[0.1, 16, 12]} />
        <meshStandardMaterial color={color} />
      </mesh>
      <Html center distanceFactor={9} zIndexRange={[10, 0]}>
        <span className="room-label muted">{t("spatial.listener")}</span>
      </Html>
    </group>
  );
}

function Speaker({ placement, radius, colors }: { placement: SpeakerPlacement; radius: number; colors: Colors }) {
  const position = scale(placement.direction as Vec3, radius);
  // Cones point along +Y by default; aim this one back at the listener so it reads as facing inward.
  const quaternion = useMemo(() => {
    const q = new THREE.Quaternion();
    const toListener = new THREE.Vector3(-position[0], -position[1], -position[2]).normalize();
    q.setFromUnitVectors(new THREE.Vector3(0, 1, 0), toListener);
    return q;
  }, [position[0], position[1], position[2]]);

  return (
    <group position={position} quaternion={quaternion}>
      <mesh>
        <coneGeometry args={[0.16, 0.34, 4]} />
        <meshStandardMaterial color={colors.muted} />
      </mesh>
      <Html center distanceFactor={9} zIndexRange={[10, 0]}>
        <span className="room-label">{placement.short_name}</span>
      </Html>
    </group>
  );
}

function DeviceObject({
  device,
  index,
  active,
  activePart,
  colors,
  onSelect,
  onSelectPart,
  register,
  registerPart,
}: {
  device: DeviceEntry;
  index: number;
  active: boolean;
  /** Which corner or leg of a bent strip carries the gizmo, or null while the whole strip is moved. */
  activePart: PathPart | null;
  colors: Colors;
  onSelect: (index: number) => void;
  onSelectPart: (index: number, part: PathPart) => void;
  register: (object: THREE.Object3D | null) => void;
  registerPart: (index: number, part: PathPart) => (object: THREE.Object3D | null) => void;
}) {
  const path = devicePath(device);
  const position = devicePosition(device) ?? [0, 0, 0];
  const extent = deviceExtent(device);
  const length = len(extent) * 2;
  const strip = isStrip(device) && length > 1e-3;
  const color = active ? colors.accent : colors.text;

  // The strip is modelled along local +X, so its orientation is exactly the rotation that takes +X onto the extent vector — which is what makes the
  // rotate gizmo's quaternion convertible straight back into an extent.
  const quaternion = useMemo(() => {
    const q = new THREE.Quaternion();
    if (strip) q.setFromUnitVectors(X_AXIS, new THREE.Vector3(...normalize(extent)));
    return q;
  }, [strip, extent[0], extent[1], extent[2]]);

  // A bent strip is stated as a corner list, so it is drawn leg by leg and each corner gets its own handle.
  if (path) {
    return (
      <PathStrip
        points={path}
        color={color}
        accent={colors.accent}
        label={deviceLabel(device)}
        index={index}
        active={active}
        activePart={activePart}
        onSelect={onSelect}
        onSelectPart={onSelectPart}
        register={register}
        registerPart={registerPart}
      />
    );
  }

  return (
    <group
      ref={register}
      position={position}
      quaternion={quaternion}
      onPointerDown={(event) => {
        event.stopPropagation();
        onSelect(index);
      }}
    >
      {strip ? (
        <mesh rotation={[0, 0, Math.PI / 2]}>
          <cylinderGeometry args={[0.055, 0.055, length, 14]} />
          <meshStandardMaterial color={color} emissive={color} emissiveIntensity={active ? 0.45 : 0.15} />
        </mesh>
      ) : (
        <mesh>
          <sphereGeometry args={[0.17, 20, 16]} />
          <meshStandardMaterial color={color} emissive={color} emissiveIntensity={active ? 0.5 : 0.18} />
        </mesh>
      )}
      <Html center distanceFactor={9} zIndexRange={[10, 0]} position={[0, strip ? 0.3 : 0.26, 0]}>
        <span className={active ? "room-label accent" : "room-label"}>{deviceLabel(device)}</span>
      </Html>
    </group>
  );
}

/**
 * A strip that runs around one or more corners: a leg per pair of corners, a ball on every corner to drag it somewhere else, a marker on the middle
 * that moves the whole thing, and a hinge per leg to turn that one run. The corners are world coordinates, so nothing here carries a transform of its
 * own — a corner the gizmo is dragging keeps the position the config says it has.
 */
function PathStrip({
  points,
  color,
  accent,
  label,
  index,
  active,
  activePart,
  onSelect,
  onSelectPart,
  register,
  registerPart,
}: {
  points: Vec3[];
  color: string;
  accent: string;
  label: string;
  index: number;
  active: boolean;
  activePart: PathPart | null;
  onSelect: (index: number) => void;
  onSelectPart: (index: number, part: PathPart) => void;
  register: (object: THREE.Object3D | null) => void;
  registerPart: (index: number, part: PathPart) => (object: THREE.Object3D | null) => void;
}) {
  const legs = useMemo(() => {
    const out: { at: number; from: Vec3; middle: Vec3; length: number; along: THREE.Quaternion; hinge: THREE.Quaternion }[] = [];
    for (let i = 1; i < points.length; i++) {
      const from = points[i - 1] as Vec3;
      const to = points[i] as Vec3;
      const step = sub(to, from);
      const length = len(step);
      if (length < 1e-4) continue;
      const direction = new THREE.Vector3(...normalize(step));
      out.push({
        at: i - 1,
        from,
        middle: scale(add(from, to), 0.5),
        length,
        // Cylinders stand along +Y, so the drawn leg is the turn that lays that axis onto the leg's own direction.
        along: new THREE.Quaternion().setFromUnitVectors(Y_AXIS, direction),
        // The hinge instead carries +X along the leg, which is the same convention a straight strip uses — the gizmo's turn then reads back as a direction.
        hinge: new THREE.Quaternion().setFromUnitVectors(X_AXIS, direction),
      });
    }
    return out;
  }, [JSON.stringify(points)]);

  const activeCorner = activePart?.kind === "corner" ? activePart.at : null;
  const activeLeg = activePart?.kind === "leg" ? activePart.at : null;
  const middle = centroid(points);
  const first = points[0] ?? [0, 0, 0];

  return (
    <group
      onPointerDown={(event) => {
        event.stopPropagation();
        onSelect(index);
      }}
    >
      {legs.map((leg) => (
        <group key={leg.at}>
          <mesh
            position={leg.middle}
            quaternion={leg.along}
            onPointerDown={(event) => {
              event.stopPropagation();
              onSelectPart(index, { kind: "leg", at: leg.at });
            }}
          >
            <cylinderGeometry args={[leg.at === activeLeg ? STRIP_RADIUS * 1.5 : STRIP_RADIUS, leg.at === activeLeg ? STRIP_RADIUS * 1.5 : STRIP_RADIUS, leg.length, 14]} />
            <meshStandardMaterial
              color={leg.at === activeLeg ? accent : color}
              emissive={leg.at === activeLeg ? accent : color}
              emissiveIntensity={active ? 0.45 : 0.15}
            />
          </mesh>
          {/* Nothing to see: it only exists so the rotate gizmo has something to sit on, at the corner this leg turns around. */}
          <group ref={registerPart(index, { kind: "leg", at: leg.at })} position={leg.from} quaternion={leg.hinge} />
        </group>
      ))}

      {points.map((point, at) => (
        <mesh
          key={at}
          ref={registerPart(index, { kind: "corner", at })}
          position={point}
          onPointerDown={(event) => {
            event.stopPropagation();
            onSelectPart(index, { kind: "corner", at });
          }}
        >
          <sphereGeometry args={[at === activeCorner ? 0.11 : 0.085, 16, 12]} />
          <meshStandardMaterial
            color={at === activeCorner ? accent : color}
            emissive={at === activeCorner ? accent : color}
            emissiveIntensity={at === activeCorner ? 0.6 : 0.2}
          />
        </mesh>
      ))}

      {/* The handle for the strip as a whole. It only shows while the strip is selected and nothing smaller has been picked. */}
      <mesh ref={register} position={middle} visible={active && activePart === null}>
        <octahedronGeometry args={[0.09]} />
        <meshStandardMaterial color={accent} emissive={accent} emissiveIntensity={0.5} />
      </mesh>

      <Html center distanceFactor={9} position={first} zIndexRange={[10, 0]}>
        <span className={active ? "room-label accent" : "room-label"}>{label}</span>
      </Html>
    </group>
  );
}
