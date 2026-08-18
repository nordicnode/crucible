// Wire types mirroring the server's JSON protocol, plus command builders.
// The client never implements game rules; it only renders fogged state and
// sends the same commands the sim validates.

export type BuildingType = "Hq" | "Refinery" | "Barracks" | "Factory" | "TechLab" | "Turret";
export type UnitType = "Harvester" | "Infantry" | "Tank" | "Artillery";
export type Stance = "Aggressive" | "Cautious" | "Hold";
export type Upgrade = "None" | "Damage" | "Hp";

export type Command =
  | { PlaceBuilding: { player: number; btype: BuildingType; tile: [number, number] } }
  | { TrainUnit: { player: number; building: number; utype: UnitType } }
  | { MoveGroup: { player: number; units: number[]; waypoint: [number, number]; stance: Stance } }
  | { SetRally: { player: number; building: number; waypoint: [number, number] } }
  | { ChooseUpgrade: { player: number; lab: number; upgrade: Upgrade } }
  | { Sell: { player: number; building: number } };

export const PLAYER = 0;

export function placeBuilding(btype: BuildingType, tile: [number, number]): Command {
  return { PlaceBuilding: { player: PLAYER, btype, tile } };
}
export function trainUnit(building: number, utype: UnitType): Command {
  return { TrainUnit: { player: PLAYER, building, utype } };
}
export function moveGroup(units: number[], waypoint: [number, number], stance: Stance = "Aggressive"): Command {
  return { MoveGroup: { player: PLAYER, units, waypoint, stance } };
}
export function setRally(building: number, waypoint: [number, number]): Command {
  return { SetRally: { player: PLAYER, building, waypoint } };
}
export function chooseUpgrade(lab: number, upgrade: Upgrade): Command {
  return { ChooseUpgrade: { player: PLAYER, lab, upgrade } };
}
export function sell(building: number): Command {
  return { Sell: { player: PLAYER, building } };
}

export interface DiffEntity {
  id: number;
  kind: string;
  owner: number;
  x: number;
  y: number;
  hp: number;
  maxHp: number;
  stale?: number;
}

export interface OreTile {
  x: number;
  y: number;
  amount: number;
}

export interface DiffEvent {
  tick: number;
  kind: string;
}

export type ServerMsg =
  | { type: "matchStart"; mapSeed: number; player: number; passable: boolean[]; hq: [number, number][] }
  | { type: "stateDiff"; tick: number; ore: number; entities: DiffEntity[]; oreTiles: OreTile[]; visible: number[]; events: DiffEvent[] }
  | { type: "matchEnd"; winner: number | null; reason: string | null; durationTicks: number; replayId: number | null };

export type ClientMsg =
  | { type: "joinMatch"; opponent: string }
  | { type: "commands"; cmds: Command[] };

export const BUILD_COSTS: Record<string, number> = {
  Refinery: 300,
  Barracks: 150,
  Factory: 250,
  TechLab: 200,
  Turret: 100,
};

export const UNIT_COSTS: Record<string, number> = {
  Harvester: 100,
  Infantry: 50,
  Tank: 150,
  Artillery: 200,
};

export const UNIT_KINDS = new Set(["Harvester", "Infantry", "Tank", "Artillery"]);
export const BUILDING_KINDS = new Set(["Hq", "Refinery", "Barracks", "Factory", "TechLab", "Turret"]);
