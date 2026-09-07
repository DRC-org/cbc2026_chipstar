#pragma once
#include <cstdint>
struct FDCAN_Registers { uint32_t PSR=0, ECR=0, CCCR=0, TXBRP=0, TXEFS=0; };
struct FDCAN_HandleTypeDef { FDCAN_Registers* Instance; };
struct FDCAN_TxHeaderTypeDef {
 uint32_t Identifier=0,IdType=0,TxFrameType=0,DataLength=0,ErrorStateIndicator=0;
 uint32_t BitRateSwitch=0,FDFormat=0,TxEventFifoControl=0,MessageMarker=0;
};
struct FDCAN_RxHeaderTypeDef { uint32_t Identifier=0,IdType=0,DataLength=0; };
constexpr int HAL_OK=0, HAL_ERROR=1;
constexpr uint32_t FDCAN_STANDARD_ID=0,FDCAN_EXTENDED_ID=1,FDCAN_DATA_FRAME=0;
constexpr uint32_t FDCAN_ESI_ACTIVE=0,FDCAN_BRS_OFF=0,FDCAN_CLASSIC_CAN=0,FDCAN_NO_TX_EVENTS=0;
constexpr uint32_t FDCAN_ACCEPT_IN_RX_FIFO0=0,FDCAN_REJECT_REMOTE=0,FDCAN_RX_FIFO0=0;
constexpr uint32_t FDCAN_PSR_BO=1,FDCAN_CCCR_INIT=1;
constexpr uint32_t FDCAN_DLC_BYTES_0=0,FDCAN_DLC_BYTES_1=1,FDCAN_DLC_BYTES_2=2,FDCAN_DLC_BYTES_3=3;
constexpr uint32_t FDCAN_DLC_BYTES_4=4,FDCAN_DLC_BYTES_5=5,FDCAN_DLC_BYTES_6=6,FDCAN_DLC_BYTES_7=7,FDCAN_DLC_BYTES_8=8;
#define CLEAR_BIT(reg, bits) ((reg) &= ~(bits))
uint32_t HAL_GetTick();
void HAL_Delay(uint32_t ms);
int HAL_FDCAN_ConfigGlobalFilter(FDCAN_HandleTypeDef*,uint32_t,uint32_t,uint32_t,uint32_t);
int HAL_FDCAN_Start(FDCAN_HandleTypeDef*);
uint32_t HAL_FDCAN_GetTxFifoFreeLevel(FDCAN_HandleTypeDef*);
int HAL_FDCAN_AddMessageToTxFifoQ(FDCAN_HandleTypeDef*,const FDCAN_TxHeaderTypeDef*,const uint8_t*);
int HAL_FDCAN_AbortTxRequest(FDCAN_HandleTypeDef*,uint32_t);
uint32_t HAL_FDCAN_GetRxFifoFillLevel(FDCAN_HandleTypeDef*,uint32_t);
int HAL_FDCAN_GetRxMessage(FDCAN_HandleTypeDef*,uint32_t,FDCAN_RxHeaderTypeDef*,uint8_t*);

struct FDCAN_TxEventFifoTypeDef { uint32_t Identifier=0, IdType=0; };
constexpr uint32_t FDCAN_STORE_TX_EVENTS=1, FDCAN_TXEFS_EFFL=7;
int HAL_FDCAN_GetTxEvent(FDCAN_HandleTypeDef*, FDCAN_TxEventFifoTypeDef*);
