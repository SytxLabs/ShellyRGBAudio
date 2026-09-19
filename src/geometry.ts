/**
 * The arithmetic between what the config stores and what the room view and the forms show.
 *
 * `devices[].extent` is a half-vector: the strip runs from `position - extent` to `position + extent`. That is the right thing to compute with and the
 * wrong thing to type in, so the UI works in length plus two angles and converts here.
 */

import type { DeviceEntry, RoomBounds } from "./bindings";

export type Vec3 = [number, number, number];

export const ZERO: Vec3 = [0, 0, 0];

export function add(a: Vec3, b: Vec3): Vec3 {return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];}
export function sub(a: Vec3, b: Vec3): Vec3 {return [a[0] - b[0], a[1] - b[1], a[2] - b[2]];}
export function scale(v: Vec3, s: number): Vec3 {return [v[0] * s, v[1] * s, v[2] * s];}
export function len(v: Vec3): number {return Math.hypot(v[0], v[1], v[2]);}
export function normalize(v: Vec3): Vec3 {
  const l = len(v);
  return l < 1e-6 ? [0, 0, 1] : scale(v, 1 / l);
}
export function round(v: Vec3, digits = 2): Vec3 {
  const f = 10 ** digits;
  return [Math.round(v[0] * f) / f, Math.round(v[1] * f) / f, Math.round(v[2] * f) / f];
}

export type StripShape = { length: number; yaw: number; pitch: number; };

const DEG = Math.PI / 180;

export function shapeToExtent(shape: StripShape): Vec3 {
  const yaw = shape.yaw * DEG;
  const pitch = shape.pitch * DEG;
  const half = Math.max(shape.length, 0) / 2;
  return round([Math.cos(pitch) * Math.sin(yaw) * half, Math.sin(pitch) * half, Math.cos(pitch) * Math.cos(yaw) * half], 3);
}

export function extentToShape(extent: Vec3): StripShape {
  const length = len(extent) * 2;
  if (length < 1e-6) return { length: 0, yaw: 0, pitch: 0 };
  const dir = normalize(extent);
  return {
    length: Math.round(length * 100) / 100,
    yaw: Math.round((Math.atan2(dir[0], dir[2]) / DEG) * 10) / 10,
    pitch: Math.round((Math.asin(Math.min(1, Math.max(-1, dir[1]))) / DEG) * 10) / 10,
  };
}

export function extentFromDirection(direction: Vec3, length: number): Vec3 {return round(scale(normalize(direction), Math.max(length, 0) / 2), 3);}

const BEND_YAW = 90;
const DEFAULT_LEG: Vec3 = [0.5, 0, 0];

export function rotateYaw(v: Vec3, degrees: number): Vec3 {
  const angle = degrees * DEG;
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  return [v[0] * cos + v[2] * sin, v[1], v[2] * cos - v[0] * sin];
}

export function dot(a: Vec3, b: Vec3): number {return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];}
export function cross(a: Vec3, b: Vec3): Vec3 {return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];}

export function centroid(points: readonly Vec3[]): Vec3 {
  if (points.length === 0) return ZERO;
  return scale(points.reduce<Vec3>((acc, p) => add(acc, p), ZERO), 1 / points.length);
}

export function pathLength(points: readonly Vec3[]): number {
  let total = 0;
  for (let i = 1; i < points.length; i++) total += len(sub(points[i] as Vec3, points[i - 1] as Vec3));
  return total;
}

export function pathFromStraight(position: Vec3, extent: Vec3): Vec3[] {
  const end = add(position, extent);
  return [sub(position, extent), end, add(end, rotateYaw(len(extent) > 1e-3 ? extent : DEFAULT_LEG, BEND_YAW))].map((p) => round(p, 3));
}

export function pathWithCorner(points: readonly Vec3[]): Vec3[] {
  const last = points[points.length - 1] ?? ZERO;
  const leg = sub(last, points[points.length - 2] ?? sub(last, DEFAULT_LEG));
  return [...points, round(add(last, rotateYaw(len(leg) > 1e-3 ? leg : DEFAULT_LEG, BEND_YAW)), 3)];
}

/** How many straight runs a corner list has. A leg is the piece between two corners, and each one points its own way. */
export function legCount(points: readonly Vec3[]): number {return Math.max(points.length - 1, 0);}

/** Turns `v` by the rotation that takes `from` onto `to`, both of which have to be unit vectors (Rodrigues). */
function turn(v: Vec3, from: Vec3, to: Vec3): Vec3 {
  const axis = cross(from, to);
  const sin = len(axis);
  const cos = dot(from, to);
  // Parallel already, or turned right around: a half turn has no unique axis, so any perpendicular one does.
  if (sin < 1e-9) {
    if (cos > 0) return v;
    const perp = normalize(cross(from, Math.abs(from[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0]));
    return sub(scale(perp, 2 * dot(perp, v)), v);
  }
  const unit = scale(axis, 1 / sin);
  return add(add(scale(v, cos), scale(cross(unit, v), sin)), scale(unit, dot(unit, v) * (1 - cos)));
}

/**
 * Points one leg of a bent strip somewhere else. The strip hinges at the leg's first corner: everything past it comes along unchanged, so turning the
 * piece that runs up the wall swings the ceiling run with it instead of tearing the strip apart.
 */
export function rotateLeg(points: readonly Vec3[], leg: number, direction: Vec3): Vec3[] {
  const pivot = points[leg];
  const end = points[leg + 1];
  if (!pivot || !end) return [...points];
  const from = normalize(sub(end, pivot));
  const to = normalize(direction);
  return points.map((p, i) => (i <= leg ? p : round(add(pivot, turn(sub(p, pivot), from, to)), 3)));
}

/** One leg as the form states it: how long that run is and which way it goes. */
export function legShape(points: readonly Vec3[], leg: number): StripShape {
  const along = sub(points[leg + 1] ?? ZERO, points[leg] ?? ZERO);
  return { ...extentToShape(along), length: Math.round(len(along) * 100) / 100 };
}

/** The other direction: a leg's length and angles written back, again hinging at its first corner. */
export function setLegShape(points: readonly Vec3[], leg: number, shape: StripShape): Vec3[] {
  const pivot = points[leg];
  if (!pivot || !points[leg + 1]) return [...points];
  const direction = normalize(shapeToExtent({ ...shape, length: 2 })); // Half of a length of 2 is the unit vector for those angles.
  const rotated = rotateLeg(points, leg, direction);
  // The turn keeps the old length, so what is left is sliding the rest of the strip along the leg until it is as long as asked for.
  const shift = sub(add(pivot, scale(direction, Math.max(shape.length, 0))), rotated[leg + 1] ?? pivot);
  return rotated.map((p, i) => (i <= leg ? p : round(add(p, shift), 3)));
}

export function straightFromPath(points: readonly Vec3[]): { position: Vec3; extent: Vec3 } {
  const first = points[0] ?? ZERO;
  const last = points[points.length - 1] ?? ZERO;
  return { position: round(scale(add(first, last), 0.5), 3), extent: round(scale(sub(last, first), 0.5), 3) };
}

export function isStrip(device: DeviceEntry): boolean {return device.form === "strip";}
export function devicePosition(device: DeviceEntry): Vec3 | null {return asVec3(device.position);}
export function deviceExtent(device: DeviceEntry): Vec3 {return asVec3(device.extent) ?? ZERO;}

export function devicePath(device: DeviceEntry): Vec3[] | null {
  if (!Array.isArray(device.path)) return null;
  const points = device.path.map(asVec3).filter((p): p is Vec3 => p !== null);
  return points.length >= 2 ? points : null;
}

export function deviceSpatiality(device: DeviceEntry): number {return typeof device.spatiality === "number" ? device.spatiality : 0;}
export function isPositioned(device: DeviceEntry): boolean {return devicePosition(device) !== null || devicePath(device) !== null;}

export function deviceLabel(device: DeviceEntry): string {
  for (const key of ["name", "host", "ip"]) {
    const value = device[key];
    if (typeof value === "string" && value.trim() !== "") return value;
  }
  return typeof device.type === "string" ? device.type : "?";
}

function asVec3(value: unknown): Vec3 | null {
  if (!Array.isArray(value) || value.length < 3) return null;
  const [x, y, z] = value;
  if (typeof x !== "number" || typeof y !== "number" || typeof z !== "number") return null;
  return [x, y, z];
}

export function roomCenter(room: RoomBounds): Vec3 {return [(room.min[0] + room.max[0]) / 2, (room.min[1] + room.max[1]) / 2, (room.min[2] + room.max[2]) / 2];}
export function roomSize(room: RoomBounds): Vec3 {return [Math.abs(room.max[0] - room.min[0]), Math.abs(room.max[1] - room.min[1]), Math.abs(room.max[2] - room.min[2])];}

export function clampToRoom(point: Vec3, room: RoomBounds): Vec3 {
  const axis = (i: 0 | 1 | 2) => {
    const lo = Math.min(room.min[i], room.max[i]);
    const hi = Math.max(room.min[i], room.max[i]);
    return Math.min(hi, Math.max(lo, point[i]));
  };
  return [axis(0), axis(1), axis(2)];
}

export function speakerRadius(room: RoomBounds): number {
  const size = roomSize(room);
  return Math.max(0.5, Math.min(size[0], size[2]) * 0.42);
}
