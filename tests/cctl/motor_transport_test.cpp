#include "doctest.h"
#include "actuator_controller.hpp"
#include "param_store.hpp"
#include <vector>
#include <cstring>

namespace {
FDCAN_Registers registers;
FDCAN_HandleTypeDef handle{&registers};
std::vector<domain::CanFrame> accepted;
uint32_t tick=0, queued=0;
bool stuck=false, fail_enqueue=false, fail_cancel=false;
uint32_t fail_identifier=0xFFFFFFFF, cancel_calls=0;
uint32_t last_event_control=0;
FDCAN_TxEventFifoTypeDef completion_event{};
void resetBus() { registers={}; accepted.clear(); tick=queued=0; stuck=fail_enqueue=fail_cancel=false;fail_identifier=0xFFFFFFFF;cancel_calls=0; }
}
uint32_t HAL_GetTick() { return tick++; }
void HAL_Delay(uint32_t ms) { tick+=ms; if(!stuck) {queued=0;registers.TXBRP=0;} }
int HAL_FDCAN_ConfigGlobalFilter(FDCAN_HandleTypeDef*,uint32_t,uint32_t,uint32_t,uint32_t) {return HAL_OK;}
int HAL_FDCAN_Start(FDCAN_HandleTypeDef*) {return HAL_OK;}
uint32_t HAL_FDCAN_GetTxFifoFreeLevel(FDCAN_HandleTypeDef*) {
 if(queued==3&&!stuck) --queued;
 registers.TXBRP=(1U<<queued)-1;
 return 3-queued;
}
int HAL_FDCAN_AddMessageToTxFifoQ(FDCAN_HandleTypeDef*,const FDCAN_TxHeaderTypeDef* header,const uint8_t* data) {
 last_event_control=header->TxEventFifoControl;
 if(fail_enqueue||queued==3||header->Identifier==fail_identifier) return HAL_ERROR;
 domain::CanFrame frame;frame.id=header->Identifier;frame.extended=header->IdType==FDCAN_EXTENDED_ID;
 frame.length=header->DataLength;std::memcpy(frame.data,data,frame.length);accepted.push_back(frame);
 ++queued;registers.TXBRP=(1U<<queued)-1;return HAL_OK;
}
int HAL_FDCAN_AbortTxRequest(FDCAN_HandleTypeDef*,uint32_t) {
 ++cancel_calls;
 if (fail_cancel) return HAL_ERROR;
 queued=0;registers.TXBRP=0;return HAL_OK;
}
uint32_t HAL_FDCAN_GetRxFifoFillLevel(FDCAN_HandleTypeDef*,uint32_t) {return 0;}
int HAL_FDCAN_GetRxMessage(FDCAN_HandleTypeDef*,uint32_t,FDCAN_RxHeaderTypeDef*,uint8_t*) {return HAL_ERROR;}
namespace param_store {
bool load(domain::Parameters&) {return false;}
bool save(const domain::Parameters&) {return true;}
bool clear() {return true;}
bool present() {return false;}
}
namespace {
void el05Position(ActuatorController& controller, float position) {
 domain::CanFrame frame;frame.id=domain::el05::buildCanId(domain::el05::comm::READ_PARAM,0x7F,0xFD);
 frame.extended=true;frame.length=8;
 domain::el05::encodeParamFloat(domain::el05::param::MECH_POS,position,frame.data);
 controller.dispatchRx(frame);
}
void el05Feedback(ActuatorController& controller, float wrapped_position) {
 domain::CanFrame frame;
 frame.id=domain::el05::buildCanId(domain::el05::comm::FEEDBACK,0x7F,0xFD);
 frame.extended=true;frame.length=8;
 const auto raw=domain::el05::floatToUint16(wrapped_position,domain::el05::POSITION_MIN,
                                            domain::el05::POSITION_MAX);
 frame.data[0]=static_cast<uint8_t>(raw>>8);frame.data[1]=static_cast<uint8_t>(raw);
 controller.dispatchRx(frame);
}
void el05Feedback(El05Motor& motor, float wrapped_position) {
 uint8_t data[8]={};
 const auto raw=domain::el05::floatToUint16(wrapped_position,domain::el05::POSITION_MIN,
                                            domain::el05::POSITION_MAX);
 data[0]=static_cast<uint8_t>(raw>>8);data[1]=static_cast<uint8_t>(raw);
 motor.onFeedback(domain::el05::buildCanId(domain::el05::comm::FEEDBACK,0x7F,0xFD),data);
}
}

TEST_CASE("3要素FIFOが空けば連続8フレームを欠落なく受け付ける") {
 resetBus();CanBus bus(&handle);uint8_t data[8]={};
 for(int n=0;n<8;++n) CHECK(bus.sendStd(0x200+n,data,8));
 CHECK(accepted.size()==8);CHECK(bus.txFailures()==0);
}
TEST_CASE("DM用コマンドと旧パラメータはCANへ送信せず拒否する") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();accepted.clear();
 CHECK_FALSE(controller.storeDmParameters());
 CHECK_FALSE(controller.readDmRegister(80));
 CHECK_FALSE(controller.writeDmRegister(21,0));
 CHECK_FALSE(controller.setParameter(11,2048));
 CHECK(accepted.empty());
}
TEST_CASE("停止中は両C620へ同じフレームでゼロ電流を送る") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 for(auto mode : {domain::RunMode::Safe,domain::RunMode::Stop}) {
  REQUIRE(controller.setMode(mode));accepted.clear();tick+=20;controller.update();
  bool sent=false;
  for(const auto& frame:accepted) if(!frame.extended) {
   CHECK(frame.id==0x200);
   for(auto byte:frame.data) CHECK(byte==0);
   sent=true;
  }
  CHECK(sent);
 }
}
TEST_CASE("FIFOが空かなくても送信待ちは有限で未送信指令を取消できる") {
 resetBus();CanBus bus(&handle);uint8_t data[8]={};stuck=true;
 for(int n=0;n<3;++n) REQUIRE(bus.sendStd(0x200+n,data,8));
 const auto start=tick;
 CHECK_FALSE(bus.sendStd(0x300,data,8));CHECK(tick-start<=4);
 CHECK(bus.txFailures()==1);CHECK(accepted.size()==3);
 CHECK(bus.discardPending());CHECK(registers.TXBRP==0);
}
TEST_CASE("Enable送信失敗はRUNと全軸の出力許可を取り消し復旧しても自動再開しない") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 el05Position(controller,0);
 REQUIRE(controller.setMode(domain::RunMode::Stop));
 REQUIRE(controller.setSlotsEnabled(7,true));
 fail_enqueue=true;
 CHECK_FALSE(controller.setMode(domain::RunMode::Run));
 CHECK(controller.mode()==domain::RunMode::Stop);CHECK(controller.enabledSlots()==0);
 CHECK_FALSE(controller.setJog(0,0.1));
 fail_enqueue=false;controller.update();
 CHECK(controller.mode()==domain::RunMode::Stop);CHECK(controller.enabledSlots()==0);
 REQUIRE(controller.setSlotsEnabled(1,true));REQUIRE(controller.setMode(domain::RunMode::Run));
 CHECK(controller.enabledSlots()==1);
}
TEST_CASE("EL05の周期位置をMECH_POSへ接続して正逆の折返しを連続化する") {
 resetBus();CanBus bus(&handle);El05Motor motor(bus,0x7F,0xFD);
 uint8_t reply[8]={};domain::el05::encodeParamFloat(domain::el05::param::MECH_POS,13.022f,reply);
 REQUIRE_FALSE(motor.onFeedback(
     domain::el05::buildCanId(domain::el05::comm::READ_PARAM,0x7F,0xFD),reply));
 REQUIRE(motor.positionReady());
 el05Feedback(motor,-12.11f);
 CHECK(motor.position()==doctest::Approx(13.03f).epsilon(0.002));
 el05Feedback(motor,-12.0f);
 CHECK(motor.position()==doctest::Approx(13.14f).epsilon(0.002));
 el05Feedback(motor,12.5f);
 CHECK(motor.position()==doctest::Approx(12.5f).epsilon(0.002));
 el05Feedback(motor,-12.5f);
 CHECK(motor.position()==doctest::Approx(12.64f).epsilon(0.002));
}
TEST_CASE("EL05の多回転位置が確立するまでslot0のRUNを拒否する") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 REQUIRE(controller.setSlotsEnabled(1,true));
 CHECK_FALSE(controller.setMode(domain::RunMode::Run));
 CHECK(controller.mode()==domain::RunMode::Safe);
 el05Position(controller,13.022f);
 REQUIRE(controller.setMode(domain::RunMode::Run));
 CHECK(controller.target(0)==doctest::Approx(13.022f));
}
TEST_CASE("EL05の折返し後もJog目標を連続座標で扱う") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 el05Position(controller,13.022f);el05Feedback(controller,-12.11f);
 REQUIRE(controller.measured(0)>13.0f);
 REQUIRE(controller.setSlotsEnabled(1,true));REQUIRE(controller.setMode(domain::RunMode::Run));
 REQUIRE(controller.setJog(0,1));tick+=20;controller.update();
  CHECK(controller.target(0)>13.0f);CHECK(controller.target(0)<=13.2f);
 el05Feedback(controller,-11.9f);tick+=20;controller.update();
 CHECK(controller.measured(0)>13.2f);
 for(int i=0;i<10;++i) {el05Feedback(controller,-11.9f);tick+=20;controller.update();}
 CHECK(controller.target(0)>13.2f);
}
TEST_CASE("EL05応答喪失後はMECH_POSを再取得するまでRUNを拒否する") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 el05Position(controller,1);el05Feedback(controller,1);
 REQUIRE(controller.setSlotsEnabled(1,true));REQUIRE(controller.setMode(domain::RunMode::Run));
 tick+=300;controller.update();
 CHECK((controller.enabledSlots()&1)==0);
 REQUIRE(controller.setMode(domain::RunMode::Stop));
 REQUIRE(controller.setSlotsEnabled(1,true));
 CHECK_FALSE(controller.setMode(domain::RunMode::Run));
 el05Position(controller,1);
 REQUIRE(controller.setMode(domain::RunMode::Run));
}
namespace {
void feedback(ActuatorController& controller, uint8_t esc, uint16_t angle, uint8_t temperature=25) {
 domain::CanFrame frame;frame.id=0x200+esc;frame.length=8;
 frame.data[0]=angle>>8;frame.data[1]=angle;frame.data[6]=temperature;
 controller.dispatchRx(frame);
}
int16_t current(const domain::CanFrame& frame, unsigned index) {
 return static_cast<int16_t>((frame.data[index*2]<<8)|frame.data[index*2+1]);
}
}
TEST_CASE("2台の目標は独立し1フレームに逆向きの電流を載せる") {
 resetBus();CanBus bus(&handle);ActuatorController c(bus);c.begin();
 feedback(c,1,1000);feedback(c,2,2000);
 REQUIRE(c.setSlotsEnabled(6,true));REQUIRE(c.setMode(domain::RunMode::Run));
 REQUIRE(c.setTarget(1,100));REQUIRE(c.setTarget(2,-100));
 accepted.clear();tick+=10;c.update();
 unsigned frames=0;
 for(const auto& frame:accepted) if(!frame.extended&&frame.id==0x200) {
  ++frames;CHECK(current(frame,0)>0);CHECK(current(frame,1)<0);
  CHECK(current(frame,2)==0);CHECK(current(frame,3)==0);
 }
 CHECK(frames==1);
 REQUIRE(c.setSlotsEnabled(4,false));
 CHECK(current(accepted.back(),0)>0);CHECK(current(accepted.back(),1)==0);
}
TEST_CASE("C620再開では停止中に手動移動した両軸の現在位置を保持する") {
 resetBus();CanBus bus(&handle);ActuatorController c(bus);c.begin();
 feedback(c,1,1000);feedback(c,2,2000);
 REQUIRE(c.setSlotsEnabled(6,true));REQUIRE(c.setMode(domain::RunMode::Run));
 REQUIRE(c.setTarget(1,100));REQUIRE(c.setTarget(2,200));
 REQUIRE(c.setMode(domain::RunMode::Stop));
 feedback(c,1,1500);feedback(c,2,3000);
 REQUIRE(c.measured(2)>40);
 accepted.clear();REQUIRE(c.setMode(domain::RunMode::Run));
 CHECK(c.target(1)==c.measured(1));CHECK(c.target(2)==c.measured(2));
 for(const auto& frame:accepted) if(!frame.extended) {
  CHECK(current(frame,0)==0);CHECK(current(frame,1)==0);
 }
 REQUIRE(c.setSlotsEnabled(4,false));feedback(c,2,3500);
 REQUIRE(c.setSlotsEnabled(4,true));
 CHECK(c.target(2)==c.measured(2));
 CHECK(current(accepted.back(),1)==0);
}
TEST_CASE("2台目の電流上限と過熱判定と応答喪失は独立する") {
 resetBus();CanBus bus(&handle);ActuatorController c(bus);c.begin();
 REQUIRE(c.setParameter(static_cast<uint8_t>(domain::ParamId::M3508Slot2MaxCurrentMa),100));
 feedback(c,1,1000);feedback(c,2,2000,90);
 CHECK(c.errorBits(1)==0);CHECK(c.errorBits(2)==domain::error_bit::OVER_TEMPERATURE);
 feedback(c,2,2000);
 REQUIRE(c.setSlotsEnabled(6,true));REQUIRE(c.setMode(domain::RunMode::Run));
 REQUIRE(c.setTarget(1,1000));REQUIRE(c.setTarget(2,1000));tick+=10;c.update();
 CHECK(c.c620CommandMilliAmp(2)<=100);CHECK(c.c620CommandMilliAmp(1)>100);
 REQUIRE(c.setParameter(static_cast<uint8_t>(domain::ParamId::M3508Slot2MaxCurrentMa),800));
 tick+=10;c.update();CHECK(c.c620CommandMilliAmp(2)>100);CHECK(c.c620CommandMilliAmp(2)<=800);
 tick+=250;feedback(c,1,1000);c.update();
 CHECK((c.enabledSlots()&2)!=0);CHECK((c.enabledSlots()&4)==0);
 CHECK((c.errorBits(2)&domain::error_bit::FEEDBACK_LOST)!=0);
 CHECK(c.c620CommandMilliAmp(2)==0);
}
TEST_CASE("ESC IDは重複と小数を拒否し別グループにも同時送信できる") {
 resetBus();CanBus bus(&handle);ActuatorController c(bus);c.begin();
 const auto id=static_cast<uint8_t>(domain::ParamId::C620Slot2EscId);
 CHECK_FALSE(c.setParameter(id,1));CHECK_FALSE(c.setParameter(id,2.5));
 REQUIRE(c.setParameter(id,5));feedback(c,1,1000);feedback(c,5,2000);
 REQUIRE(c.setSlotsEnabled(6,true));REQUIRE(c.setMode(domain::RunMode::Run));
 REQUIRE(c.setTarget(1,100));REQUIRE(c.setTarget(2,100));accepted.clear();tick+=10;c.update();
 unsigned mask=0;
 for(const auto& frame:accepted) if(!frame.extended) {
  if(frame.id==0x200) mask|=1;
  if(frame.id==0x1FF) mask|=2;
  CHECK(current(frame,0)>0);
 }
 CHECK(mask==3);CHECK_FALSE(c.setParameter(id,6));
}
TEST_CASE("EL05速度上限はPP用のVEL_MAXへ書く") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();accepted.clear();
 REQUIRE(controller.setParameter(static_cast<uint8_t>(domain::ParamId::El05LimitSpd),0.75f));
 REQUIRE(accepted.size()==1);
 CHECK(accepted[0].data[0]==0x24);CHECK(accepted[0].data[1]==0x70);
 float value=0;std::memcpy(&value,&accepted[0].data[4],4);CHECK(value==doctest::Approx(0.75));
}

TEST_CASE("EL05は出力を有効化するたびにPPモードと制限値を復元する") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 el05Position(controller,0);
 REQUIRE(controller.setSlotsEnabled(1,true));accepted.clear();
 REQUIRE(controller.setMode(domain::RunMode::Run));
 auto countRunModeWrites=[] {
  unsigned count=0;
  for(const auto& frame:accepted) {
   if(frame.extended && domain::el05::commType(frame.id)==domain::el05::comm::WRITE_PARAM &&
      frame.data[0]==0x05 && frame.data[1]==0x70 && frame.data[4]==1) ++count;
  }
  return count;
 };
 CHECK(countRunModeWrites()==1);

 // 他軸の有効状態を変えただけなら、動作中のEL05を再初期化しない。
 accepted.clear();REQUIRE(controller.setSlotsEnabled(2,true));
 CHECK(countRunModeWrites()==0);

 // STOP中にEL05だけ再通電された場合も、次のRUNで設定を復元する。
 REQUIRE(controller.setMode(domain::RunMode::Stop));accepted.clear();
 REQUIRE(controller.setMode(domain::RunMode::Run));
 CHECK(countRunModeWrites()==1);
}

TEST_CASE("周期指令と再初期化の送信失敗も停止側へ戻す") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 el05Position(controller,0);
 REQUIRE(controller.setSlotsEnabled(1,true));REQUIRE(controller.setMode(domain::RunMode::Run));
 fail_enqueue=true;tick+=20;controller.update();
 CHECK(controller.mode()==domain::RunMode::Stop);CHECK(controller.enabledSlots()==0);
 fail_enqueue=false;controller.update();REQUIRE(controller.setMode(domain::RunMode::Safe));
 fail_enqueue=true;CHECK_FALSE(controller.reinitialize(1));CHECK(controller.mode()==domain::RunMode::Stop);
}
TEST_CASE("EL05設定送信失敗時は設定値を巻き戻して再試行できる") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 const auto id=static_cast<uint8_t>(domain::ParamId::El05LimitSpd);
 const auto old=controller.parameters().get(id);
 fail_enqueue=true;CHECK_FALSE(controller.setParameter(id,0.75f));
 CHECK(controller.parameters().get(id)==old);
 fail_enqueue=false;controller.update();REQUIRE(controller.setMode(domain::RunMode::Safe));
 CHECK(controller.setParameter(id,0.75f));
}

TEST_CASE("停止時の取消失敗でも内部出力を切り停止再送までRUNを拒否する") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 el05Position(controller,0);
 REQUIRE(controller.setSlotsEnabled(7,true));REQUIRE(controller.setMode(domain::RunMode::Run));
 fail_cancel=true;
 CHECK_FALSE(controller.setMode(domain::RunMode::Stop));
 CHECK(controller.mode()==domain::RunMode::Stop);CHECK(controller.enabledSlots()==0);
 CHECK_FALSE(controller.setMode(domain::RunMode::Run));
 fail_cancel=false;controller.update();
 CHECK(controller.enabledSlots()==0);
}

TEST_CASE("停止再送は既に受付済みの停止フレームを取消も重複送信もしない") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 el05Position(controller,0);
 REQUIRE(controller.setSlotsEnabled(7,true));
 fail_enqueue=true;CHECK_FALSE(controller.setMode(domain::RunMode::Run));
 fail_enqueue=false;fail_identifier=0x200;accepted.clear();
 controller.update();
 REQUIRE(accepted.size()==1);
 const auto cancels=cancel_calls;
 fail_identifier=0xFFFFFFFF;
 controller.update();
 CHECK(cancel_calls==cancels);REQUIRE(accepted.size()==2);
 CHECK(accepted.back().id==0x200);
 for(auto byte:accepted.back().data) CHECK(byte==0);
 CHECK(controller.mode()==domain::RunMode::Stop);CHECK(controller.enabledSlots()==0);
}
TEST_CASE("同じRUNの再要求ではEnableを再送しない") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 el05Position(controller,0);
 REQUIRE(controller.setSlotsEnabled(5,true));REQUIRE(controller.setMode(domain::RunMode::Run));
 accepted.clear();CHECK(controller.setMode(domain::RunMode::Run));CHECK(accepted.empty());
}

int HAL_FDCAN_GetTxEvent(FDCAN_HandleTypeDef*, FDCAN_TxEventFifoTypeDef* event) {
  if (!registers.TXEFS) return HAL_ERROR;
  *event = completion_event;
  registers.TXEFS = 0;
  return HAL_OK;
}

TEST_CASE("識別要求の受付とCAN送信完了を区別する") {
  resetBus();
  CanBus bus(&handle);
  uint8_t data[8] = {};
  REQUIRE(bus.sendExt(0xFD7F, data, 8, true));
  CHECK(last_event_control == FDCAN_STORE_TX_EVENTS);
  uint32_t id = 0;
  bool extended = false;
  CHECK_FALSE(bus.receiveTxCompletion(id, extended));
  completion_event.Identifier = 0xFD7F;
  completion_event.IdType = FDCAN_EXTENDED_ID;
  registers.TXEFS = 1;
  REQUIRE(bus.receiveTxCompletion(id, extended));
  CHECK(id == 0xFD7F);
  CHECK(extended);
  CHECK_FALSE(bus.receiveTxCompletion(id, extended));
  REQUIRE(bus.sendStd(0x200, data, 8));
  CHECK(last_event_control == FDCAN_NO_TX_EVENTS);
}

TEST_CASE("EL05読出しはマニュアルのloc_kp要求と応答の実例に一致する") {
  resetBus();
  CanBus bus(&handle);
  El05Motor motor(bus, 0x7F, 0xFD);
  REQUIRE(motor.requestParam(0x701E));
  REQUIRE(accepted.size() == 1);
  const uint8_t request[8] = {0x1E, 0x70, 0, 0, 0, 0, 0, 0};
  CHECK(accepted[0].extended);
  CHECK(accepted[0].id == 0x1100FD7F);
  CHECK(std::memcmp(accepted[0].data, request, 8) == 0);

  // EL05 manual 4.1.16: motor 0x7F -> host 0xFD, loc_kp = 30.
  const uint8_t reply[8] = {0x1E, 0x70, 0, 0, 0, 0, 0xF0, 0x41};
  CHECK_FALSE(motor.onFeedback(0x11007FFD, reply));
  CHECK(motor.lastParamFloat() == doctest::Approx(30.0f));
  uint16_t index = 0;
  uint32_t raw = 0;
  REQUIRE(motor.takeParamReply(index, raw));
  CHECK(index == 0x701E);
  CHECK(raw == 0x41F00000);
  CHECK_FALSE(motor.takeParamReply(index, raw));

  CHECK_FALSE(motor.onFeedback(0x11007EFD, reply));
  CHECK_FALSE(motor.onFeedback(0x11007FFC, reply));
  CHECK_FALSE(motor.takeParamReply(index, raw));
}
