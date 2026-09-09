#include "doctest.h"
#include "sts3215.hpp"
#include "servo_service.hpp"
#include <array>
#include <vector>
#include <cstring>
GPIO_TypeDef fake_gpio;
namespace {
DMA_HandleTypeDef dma;
UART_HandleTypeDef uart{&dma};
uint8_t* rx=nullptr;
uint16_t head=0,size=0;
uint32_t ticks=0;
std::array<uint8_t,256> regs;
std::vector<std::vector<uint8_t>> sent;
bool silent=false, noise=false, ignore_write=false;
unsigned delayed_reads=0, dropped_writes=0, dropped_reads=0;
std::vector<uint8_t> pending_write;
uint8_t pending_address=0;
uint8_t fault=0;
bool receiver_stalled=false, fail_start=false, receive_error=false;
bool rx_idle_high=true;
unsigned receiver_starts=0;
void reset() { regs.fill(0);regs[56]=0x34;regs[57]=0x08;sent.clear();ticks=0;silent=noise=ignore_write=false;fault=0;uart.ErrorCode=0;receiver_stalled=fail_start=receive_error=false;rx_idle_high=true;receiver_starts=0;delayed_reads=0;dropped_writes=0;dropped_reads=0;pending_write.clear(); }
void append(const std::vector<uint8_t>& bytes) { for(auto b:bytes) {rx[head]=b;head=(head+1)%size;} dma.count=size-head; }
void status(uint8_t id, const std::vector<uint8_t>& data, uint8_t error=0) {
 std::vector<uint8_t> p{255,255,id,static_cast<uint8_t>(data.size()+2),error};
 p.insert(p.end(),data.begin(),data.end());
 p.push_back(domain::sts3215::checksum(p.data()+2,p.size()-2));append(p);
}
}
uint32_t HAL_GetTick() { return ticks++/100; }
void HAL_Delay(uint32_t ms) { ticks += ms * 100; }
HAL_StatusTypeDef HAL_UART_Receive_DMA(UART_HandleTypeDef*,uint8_t* data,uint16_t n) {
 ++receiver_starts;
 if(fail_start)return HAL_ERROR;
 receiver_stalled=false;rx=data;size=n;head=0;dma.count=n;return HAL_OK;
}
HAL_StatusTypeDef HAL_UART_AbortReceive(UART_HandleTypeDef*) {uart.ErrorCode=0;return HAL_OK;}
GPIO_PinState HAL_GPIO_ReadPin(GPIO_TypeDef*, uint16_t) {return rx_idle_high?GPIO_PIN_SET:GPIO_PIN_RESET;}
HAL_StatusTypeDef HAL_UART_Receive(UART_HandleTypeDef*,uint8_t*,uint16_t,uint32_t) {return HAL_TIMEOUT;}
HAL_StatusTypeDef HAL_UART_Transmit(UART_HandleTypeDef*,uint8_t* p,uint16_t n,uint32_t) {
 sent.emplace_back(p,p+n);
 if(receive_error) {uart.ErrorCode=1;return HAL_OK;}
 if(receiver_stalled)return HAL_OK;
 if(silent) return HAL_OK;
 if(p[4]==0x83) {
  if(dropped_writes) {--dropped_writes;return HAL_OK;}
  if(!ignore_write) {
   if(delayed_reads) {pending_address=p[5];pending_write.assign(p+8,p+8+p[6]);}
   else std::memcpy(regs.data()+p[5],p+8,p[6]);
  }
 } else if(p[4]==3) {
  if(!ignore_write) std::memcpy(regs.data()+p[5],p+6,n-7);
  if(p[2]!=254)status(p[2],{},fault);
 } else if(p[4]==2) {
  if(dropped_reads) {--dropped_reads;return HAL_OK;}
  if(noise) {append({0,7,255});status(9,{});status(p[2],{});noise=false;}
  status(p[2],std::vector<uint8_t>(regs.begin()+p[5],regs.begin()+p[5]+p[6]),fault);
  if(delayed_reads && --delayed_reads==0 && !pending_write.empty()) {
   std::memcpy(regs.data()+pending_address,pending_write.data(),pending_write.size());
   pending_write.clear();
  }
 } else if(p[4]==1)status(p[2],{},fault);
 return HAL_OK;
}
TEST_CASE("循環DMAで位置応答を読み古いACKと他IDを読み飛ばす") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 for(unsigned i=0;i<50;++i) {noise=true;uint16_t position=0;REQUIRE(bus.readPosition(1,position)==Sts3215::Result::Ok);CHECK(position==2100);}
}
TEST_CASE("待機時LOWのSTS信号線は電源・配線異常として送信前に識別する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 rx_idle_high=false;uint16_t position=0;
 CHECK(bus.readPosition(1,position)==Sts3215::Result::BusLow);
 CHECK(sent.empty());CHECK(bus.lastHalStatus()==HAL_OK);
}
TEST_CASE("受信停止のタイムアウト後はDMAを張り直し再開失敗も再試行する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 receiver_stalled=true;uint16_t position=0;
 REQUIRE(bus.readPosition(1,position)==Sts3215::Result::Ok);
 CHECK(position==2100);CHECK(receiver_starts==2);
 silent=true;
 CHECK(bus.readPosition(1,position)==Sts3215::Result::Timeout);
 fail_start=true;const auto transmissions=sent.size();
 CHECK(bus.readPosition(1,position)==Sts3215::Result::HalError);
 CHECK(sent.size()==transmissions);
 fail_start=false;silent=false;
 REQUIRE(bus.readPosition(1,position)==Sts3215::Result::Ok);
 CHECK(position==2100);CHECK(receiver_starts==8);
}
TEST_CASE("受信UART異常はHAL成功と誤表示せず次の読取りで復旧する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 receive_error=true;uint16_t position=0;
 CHECK(bus.readPosition(1,position)==Sts3215::Result::HalError);
 CHECK(bus.lastHalStatus()==HAL_ERROR);
 receive_error=false;
 REQUIRE(bus.readPosition(1,position)==Sts3215::Result::Ok);
 CHECK(receiver_starts==4);CHECK(position==2100);
}
TEST_CASE("一時的な読取り応答の欠落は受信再初期化と3回目のREADで回復する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 dropped_reads=2;uint16_t position=0;
 REQUIRE(bus.readPosition(1,position)==Sts3215::Result::Ok);
 CHECK(position==2100);CHECK(sent.size()==3);CHECK(receiver_starts==3);
 for(const auto& packet:sent) CHECK(packet[4]==2);
 CHECK(ticks>=5000);
}
TEST_CASE("応答なしや読戻し不一致を送信成功として扱わない") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 silent=true;CHECK(bus.setTorque(1,true)==Sts3215::Result::Timeout);
 silent=false;ignore_write=true;CHECK(bus.setTorque(1,true)==Sts3215::Result::ReadbackMismatch);
 ignore_write=false;CHECK(bus.setTorque(1,true)==Sts3215::Result::Ok);
 CHECK(regs[40]==1);CHECK(sent[sent.size()-2][4]==0x83);
}
TEST_CASE("目標の反映遅延は読戻しだけを再試行し書込みを重複させない") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 delayed_reads=2;
 REQUIRE(bus.setTarget({1,10,2048,0,100})==Sts3215::Result::Ok);
 unsigned writes=0,reads=0;
 for(const auto& p:sent) {writes+=p[4]==0x83;reads+=p[4]==2;}
 CHECK(writes==1);CHECK(reads==3);CHECK(ticks>=200);
}
TEST_CASE("継続する不一致は上限で失敗し最初の不一致バイトを残す") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ignore_write=true;
 REQUIRE(bus.setTarget({1,0,2048,0,100})==Sts3215::Result::ReadbackMismatch);
 const auto mismatch=bus.lastReadbackMismatch();
 CHECK(mismatch.address==43);CHECK(mismatch.expected==8);CHECK(mismatch.actual==0);
 unsigned writes=0,reads=0;
 for(const auto& p:sent) {writes+=p[4]==0x83;reads+=p[4]==2;}
 CHECK(writes==3);CHECK(reads==9);
}
TEST_CASE("失われた絶対位置指令は同じ目標を再送して照合する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 dropped_writes=2;
 REQUIRE(bus.setTarget({1,10,2048,0,100})==Sts3215::Result::Ok);
 unsigned writes=0;
 for(const auto& p:sent) if(p[4]==0x83) {
  ++writes;CHECK(p[8]==10);CHECK(p[9]==0);CHECK(p[10]==8);
 }
 CHECK(writes==3);CHECK(regs[42]==0);CHECK(regs[43]==8);
}
TEST_CASE("一般書込みは読戻し不一致でも再送しない") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ignore_write=true;const uint8_t value=1;
 CHECK(bus.writeVerified(1,55,&value,1)==Sts3215::Result::ReadbackMismatch);
 unsigned writes=0;for(const auto& p:sent) writes+=p[4]==0x83;
 CHECK(writes==1);
}
TEST_CASE("出力解除の取りこぼしも読戻しを確認して再送する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 regs[40]=1;dropped_writes=2;
 REQUIRE(bus.setTorque(1,false)==Sts3215::Result::Ok);
 CHECK(regs[40]==0);
 unsigned writes=0;for(const auto& p:sent) writes+=p[4]==0x83;
 CHECK(writes==3);
}
TEST_CASE("読戻し時のサーボ保護異常は再試行しない") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 fault=4;
 CHECK(bus.setTarget({1,10,2048,0,100})==Sts3215::Result::ServoError);
 CHECK(sent.size()==2);CHECK(bus.lastServoError()==4);
}
TEST_CASE("現在位置の目標を書いてからトルクを有効化する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 REQUIRE(bus.preparePosition(1)==Sts3215::Result::Ok);
 CHECK(regs[42]==0x34);CHECK(regs[43]==0x08);CHECK(regs[40]==1);
 unsigned target=999,enable=999;
 for(unsigned i=0;i<sent.size();++i) if(sent[i][4]==0x83) {
  if(sent[i][5]==41) target=i;
  if(sent[i][5]==40&&sent[i][8]==1)enable=i;
 }
 CHECK(target<enable);
}
TEST_CASE("速度モードやサーボ異常では位置操作を開始しない") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 regs[33]=1;CHECK(bus.preparePosition(1)==Sts3215::Result::UnsupportedMode);CHECK(regs[40]==0);
 regs[33]=0;fault=4;CHECK(bus.preparePosition(1)==Sts3215::Result::ServoError);CHECK(bus.lastServoError()==4);
 fault=0;uint16_t pos=0;CHECK(bus.readPosition(1,pos)==Sts3215::Result::Ok);CHECK(bus.lastServoError()==0);
 regs[56]=0x00;regs[57]=0x98;CHECK(bus.readPosition(1,pos)==Sts3215::Result::Ok);CHECK(pos==0x9800);
 regs[56]=0x01;regs[57]=0xF0;CHECK(bus.readPosition(1,pos)==Sts3215::Result::ProtocolError);
}
TEST_CASE("目標の符号化が公式SDKの7byte指令と一致する") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 REQUIRE(bus.setTarget({1,50,2048,0,500})==Sts3215::Result::Ok);
 const std::vector<uint8_t> expected{255,255,254,12,131,41,7,1,50,0,8,0,0,244,1,18};
 CHECK(sent[0]==expected);
}
TEST_CASE("通常位置指令も正負7回転の多回転値を送れる") {
 reset();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 REQUIRE(bus.setTarget({1,50,0x1800,0,0})==Sts3215::Result::Ok);
 CHECK(regs[42]==0x00);CHECK(regs[43]==0x18);
 REQUIRE(bus.setTarget({1,50,0x9800,0,0})==Sts3215::Result::Ok);
 CHECK(regs[42]==0x00);CHECK(regs[43]==0x98);
 CHECK(bus.setTarget({1,50,0x7001,0,0})==Sts3215::Result::ArgumentError);
 CHECK(bus.setTarget({1,50,0x8000,0,0})==Sts3215::Result::ArgumentError);
}

namespace {
std::vector<std::array<uint8_t,8>> replies;
void emitService(uint16_t, const uint8_t* data) { std::array<uint8_t,8> p{};std::memcpy(p.data(),data,8);replies.push_back(p); }
bool changeBaud(uint32_t) { return true; }
}
TEST_CASE("一括監視は受信停止から復旧し全4分割データと成功応答を返す") {
 reset();replies.clear();Sts3215 bus(&uart,20,false);
 REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 receiver_stalled=true;
 const uint8_t monitor[8]={1,24,1,7,0,0,0,0};
 service.handle(monitor,false);
 REQUIRE(replies.size()==5);
 CHECK(replies.back()[4]==0);CHECK(receiver_starts==2);
 CHECK(replies[0][4]==0x34);CHECK(replies[0][5]==0x08);
 for(unsigned i=0;i<4;++i) CHECK(replies[i][3]==i);
}
TEST_CASE("相対ステップのEXECUTE再送で二度動かさず停止で待機目標を破棄する") {
 reset();replies.clear();regs[33]=3;
 Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t stage[8]={1,22,1,20,0x80,100,0,100};
 const uint8_t execute[8]={1,23,1,7,0,0,0,0};
 service.handle(stage,true);service.handle(execute,true);
 REQUIRE(replies.back()[4]==0);
 const auto transmissions=sent.size();
 service.handle(execute,true);CHECK(sent.size()==transmissions);
 REQUIRE(service.stop());CHECK(regs[40]==0);CHECK_FALSE(service.active());
 const uint8_t empty[8]={1,23,1,8,0,0,0,0};
 service.handle(empty,true);CHECK(replies.back()[4]!=0);
}
TEST_CASE("相対ステップは読戻し不一致でも目標を書き直さず停止する") {
 reset();replies.clear();regs[33]=3;regs[40]=1;dropped_writes=1;
 Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t stage[8]={1,22,1,20,0,10,0,100};
 const uint8_t execute[8]={1,23,1,7,0,0,0,0};
 service.handle(stage,true);service.handle(execute,true);
 CHECK(replies.back()[4]==static_cast<uint8_t>(Sts3215::Result::ReadbackMismatch));
 CHECK_FALSE(service.active());CHECK(regs[40]==0);
 unsigned targets=0;for(const auto& p:sent) targets+=p[4]==0x83&&p[5]==41;
 CHECK(targets==1);
}
TEST_CASE("保存設定は停止中だけ変更しモード3の角度制限とロックを読戻す") {
 reset();replies.clear();Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t mode[8]={1,21,1,3,33,1,0,3};
 service.handle(mode,true);CHECK(replies.back()[4]!=0);CHECK(regs[33]==0);
 regs[11]=255;regs[12]=15;
 service.handle(mode,false);REQUIRE(replies.back()[4]==0);
 CHECK(regs[33]==3);CHECK(regs[11]==0);CHECK(regs[12]==0);CHECK(regs[55]==1);CHECK(regs[40]==0);
 const uint8_t arbitrary[8]={1,21,1,4,40,1,0,1};
 service.handle(arbitrary,false);CHECK(replies.back()[4]!=0);CHECK(regs[40]==0);
}

TEST_CASE("多回転位置は角度上下限0のときだけ許可し符号付き現在位置から開始する") {
 reset();replies.clear();regs[56]=0x88;regs[57]=0x13;
 Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t stage[8]={1,22,1,20,0x93,0x88,0,100};
 const uint8_t execute[8]={1,23,1,7,0,0,0,0};
 regs[11]=255;regs[12]=15;
 service.handle(stage,true);service.handle(execute,true);CHECK(replies.back()[4]!=0);CHECK(regs[40]==0);
 regs[11]=0;regs[12]=0;
 service.handle(stage,true);service.handle(execute,true);REQUIRE(replies.back()[4]==0);
 CHECK(regs[42]==0x88);CHECK(regs[43]==0x93);
 REQUIRE(service.stop());
}

TEST_CASE("現在位置が設定角度範囲外ならトルクを有効化しない") {
 reset();replies.clear();regs[11]=0xe8;regs[12]=3;
 Sts3215 bus(&uart,20,false);REQUIRE(bus.startReceiver()==Sts3215::Result::Ok);
 ServoService service(bus,emitService,changeBaud);
 const uint8_t stage[8]={1,22,1,20,1,244,0,100};
 const uint8_t execute[8]={1,23,1,7,0,0,0,0};
 service.handle(stage,true);service.handle(execute,true);
 CHECK(replies.back()[4]!=0);CHECK(regs[40]==0);
 for(const auto& p:sent) CHECK_FALSE((p[4]==0x83 && p[5]==40 && p[8]==1));
}
