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

TEST_CASE("3要素FIFOが空けば連続8フレームを欠落なく受け付ける") {
 resetBus();CanBus bus(&handle);uint8_t data[8]={};
 for(int n=0;n<8;++n) CHECK(bus.sendStd(0x200+n,data,8));
 CHECK(accepted.size()==8);CHECK(bus.txFailures()==0);
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
TEST_CASE("開始シーケンスはEL05とDMのEnableを送信する") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();accepted.clear();
 REQUIRE(controller.setMode(domain::RunMode::Stop));
 REQUIRE(controller.setSlotsEnabled(7,false));REQUIRE(controller.setSlotsEnabled(5,true));
 REQUIRE(controller.setMode(domain::RunMode::Run));
 bool el05=false,dm=false;
 for(const auto& f:accepted) {
  el05|=f.extended&&domain::el05::commType(f.id)==3;
  dm|=!f.extended&&f.id==9&&f.data[7]==0xFC;
 }
 CHECK(el05);CHECK(dm);
}
TEST_CASE("EL05速度上限はPP用のVEL_MAXへ書く") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();accepted.clear();
 REQUIRE(controller.setParameter(static_cast<uint8_t>(domain::ParamId::El05LimitSpd),0.75f));
 REQUIRE(accepted.size()==1);
 CHECK(accepted[0].data[0]==0x24);CHECK(accepted[0].data[1]==0x70);
 float value=0;std::memcpy(&value,&accepted[0].data[4],4);CHECK(value==doctest::Approx(0.75));
}

TEST_CASE("周期指令と再初期化の送信失敗も停止側へ戻す") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
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
 REQUIRE(controller.setSlotsEnabled(7,true));
 fail_enqueue=true;CHECK_FALSE(controller.setMode(domain::RunMode::Run));
 fail_enqueue=false;fail_identifier=9;accepted.clear();
 controller.update();
 REQUIRE(accepted.size()==2);
 const auto cancels=cancel_calls;
 fail_identifier=0xFFFFFFFF;
 controller.update();
 CHECK(cancel_calls==cancels);REQUIRE(accepted.size()==3);
 CHECK(accepted.back().id==9);CHECK(accepted.back().data[7]==0xFD);
 CHECK(controller.mode()==domain::RunMode::Stop);CHECK(controller.enabledSlots()==0);
}
TEST_CASE("同じRUNの再要求ではEnableを再送しない") {
 resetBus();CanBus bus(&handle);ActuatorController controller(bus);controller.begin();
 REQUIRE(controller.setSlotsEnabled(5,true));REQUIRE(controller.setMode(domain::RunMode::Run));
 accepted.clear();CHECK(controller.setMode(domain::RunMode::Run));CHECK(accepted.empty());
}
