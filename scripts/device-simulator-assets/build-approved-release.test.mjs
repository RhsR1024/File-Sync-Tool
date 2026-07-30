import assert from "node:assert/strict";
import test from "node:test";

import {
  parseApprovedAlarmDefinitions,
  parseApprovedDeviceIdentity,
} from "./build-approved-release.mjs";

const source = `
结构化相机:
  - {AlarmType: '车辆抓拍', serverSupport: ["VMS系列", "UMS"], picName: 'car', picData: 'StructStruct\\\\StructureCar.json', picData-vms: 'StructStruct\\\\StructureCar-vms.json',picHeader: '/LAPI/V1.0/System/Event/Notification/Structure',alarmData: 'StructStruct\\\\CarAlarm.json',alarmTypeOff: 'CarCleared',alarmProtocol: 'V1.0'}
  - {AlarmType: '人脸抓拍', serverSupport: ["UMS"], Desc: "FaceDetection", picName: 'face', picData: 'StructStruct\\Event_Notification.json',alarmProtocol: 'V1.1'}
  - {AlarmType: '门磁报警', serverSupport: ["EZAccess"], alarmData: 'StructStruct\\UnInitialOpen.json',alarmProtocol: 'V1.0'}
`;

test("extracts approved alarm metadata without interpreting platform claims as verification", () => {
  const definitions = parseApprovedAlarmDefinitions(source, "ipc-structured");
  // The EZAccess-only alarm is dropped; only UMS-capable alarms survive.
  assert.equal(definitions.length, 2);
  assert.deepEqual(definitions[0].platforms, ["ums"]);
  assert.equal(definitions[0].structure_template, "object/StructStruct/StructureCar.json");
  assert.equal("structure_template_vms" in definitions[0], false);
  assert.equal(definitions[0].recovery_event_type, "CarCleared");
  assert.equal(definitions[0].evidence.status, "reviewed_static");
  assert.equal(definitions[1].id, "face-detection");
  assert.deepEqual(definitions[1].platforms, ["ums"]);
});

test("normalizes repeated legacy separators and restores the structured object folder", () => {
  const [car] = parseApprovedAlarmDefinitions(source, "ipc-structured");
  assert.equal(car.alarm_template, "object/StructStruct/CarAlarm.json");
});

test("extracts immutable identity facts from the approved device type source", () => {
  const identity = parseApprovedDeviceIdentity(`
结构化相机:
  dev_type: HIC6881-IR@X38-L-WSGB-VC
  dev_version: QIPC-B2201_0221
  nick_name: STRUCT
  dev_typeenum: 0
`, "结构化相机");
  assert.deepEqual(identity, {
    model: "HIC6881-IR@X38-L-WSGB-VC",
    firmware_version: "QIPC-B2201_0221",
    nickname: "STRUCT",
    device_type_enum: 0,
  });
});
