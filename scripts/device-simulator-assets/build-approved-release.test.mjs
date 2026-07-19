import assert from "node:assert/strict";
import test from "node:test";

import {
  parseApprovedAlarmDefinitions,
  parseApprovedDeviceIdentity,
} from "./build-approved-release.mjs";

const source = `
智能相机:
  - {AlarmType: '越界检测', serverSupport: ["VMS系列", "UMS"], picName: 'crossline', picData: 'SmartStruct\\\\StructureCrossLine.json', picData-vms: 'SmartStruct\\\\StructureCrossLine-vms.json',picHeader: '/LAPI/V1.0/System/Event/Notification/Structure',alarmData: 'SmartStruct\\\\CrossLineAlarm.json',alarmTypeOff: 'CrossLineCleared',alarmProtocol: 'V1.0'}
  - {AlarmType: '鼠患检测', serverSupport: ["UMS"], Desc: "MouseDetection", picName: 'alarmtype1.1', picData: 'SmartStruct\\Event_Notification.json',alarmProtocol: 'V1.1'}

普通NVR:
  - {AlarmType: '运动检测', serverSupport: ["VMS系列", "UMS"], Desc: "MotionAlarmOn", alarmData: 'NormalStruct\\NormalAlarm.json', type: "channel", alarmTypeOff: "MotionAlarmOff",alarmProtocol: 'V1.0'}

自定义报警相机:
  - {AlarmType: '新人脸检测', EventType: 'NewObjectIsRecognized', serverSupport: ["VMS系列", "UMS"], EventAlarm: 'CustomAlarm_Pic.json',picName: 'custom',issupportpic: 1,alarmProtocol: 'V1.1'}
`;

test("extracts approved alarm metadata without interpreting platform claims as verification", () => {
  const definitions = parseApprovedAlarmDefinitions(source, "ipc-smart");
  assert.equal(definitions.length, 2);
  assert.deepEqual(definitions[0].platforms, ["vms", "ums"]);
  assert.equal(definitions[0].structure_template, "object/SmartStruct/StructureCrossLine.json");
  assert.equal(definitions[0].recovery_event_type, "CrossLineCleared");
  assert.equal(definitions[0].evidence.status, "reviewed_static");
  assert.equal(definitions[1].id, "mouse-detection");
  assert.deepEqual(definitions[1].platforms, ["ums"]);
});

test("ordinary NVR definitions retain source type and recovery event", () => {
  const [definition] = parseApprovedAlarmDefinitions(source, "nvr-common");
  assert.equal(definition.id, "motion-alarm-on");
  assert.equal(definition.source_type, "channel");
  assert.equal(definition.recovery_event_type, "MotionAlarmOff");
});

test("normalizes repeated legacy separators and restores the custom object folder", () => {
  const [smart] = parseApprovedAlarmDefinitions(source, "ipc-smart");
  const [custom] = parseApprovedAlarmDefinitions(source, "ipc-custom");
  assert.equal(smart.alarm_template, "object/SmartStruct/CrossLineAlarm.json");
  assert.equal(custom.alarm_template, "object/CustomStruct/CustomAlarm_Pic.json");
});

test("extracts immutable identity facts from the approved device type source", () => {
  const identity = parseApprovedDeviceIdentity(`
智能相机:
  dev_type: IPC3615SB-ADF28KM-I0
  dev_version: GIPC-B6202.SMD-20220629.220629
  nick_name: SMART
  dev_typeenum: 0
`, "智能相机");
  assert.deepEqual(identity, {
    model: "IPC3615SB-ADF28KM-I0",
    firmware_version: "GIPC-B6202.SMD-20220629.220629",
    nickname: "SMART",
    device_type_enum: 0,
  });
});
