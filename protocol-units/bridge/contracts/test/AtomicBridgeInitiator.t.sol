// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.22;
pragma abicoder v2;

import {Test, console} from "forge-std/Test.sol";
import {AtomicBridgeInitiator, IAtomicBridgeInitiator, OwnableUpgradeable} from "../src/AtomicBridgeInitiator.sol";
import {ProxyAdmin} from "@openzeppelin/contracts/proxy/transparent/ProxyAdmin.sol";
import {TransparentUpgradeableProxy} from "@openzeppelin/contracts/proxy/transparent/TransparentUpgradeableProxy.sol";
import {IWETH9} from "../src/IWETH9.sol";
import {console} from "forge-std/console.sol";


contract AtomicBridgeInitiatorWethTest is Test {
    AtomicBridgeInitiator public atomicBridgeInitiatorImplementation;
    IWETH9 public weth;
    ProxyAdmin public proxyAdmin;
    TransparentUpgradeableProxy public proxy;
    AtomicBridgeInitiator public atomicBridgeInitiator;

    address public originator =  address(1);
    // convert to bytes32
    bytes32 public recipient = keccak256(abi.encodePacked(address(2)));
    bytes32 public hashLock = keccak256(abi.encodePacked("secret"));
    uint256 public amount = 1 ether;
    uint256 public timeLock = 100;

    function setUp() public {
        // Sepolia WETH9 address
        address wethAddress = 0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14;
        weth = IWETH9(wethAddress);

        // generate random address for each test
        originator = vm.addr(uint256(keccak256(abi.encodePacked(block.number, block.prevrandao))));

        // Deploy the AtomicBridgeInitiator contract with the WETH address
        atomicBridgeInitiatorImplementation = new AtomicBridgeInitiator();
        proxyAdmin = new ProxyAdmin(msg.sender);
        proxy = new TransparentUpgradeableProxy(
            address(atomicBridgeInitiatorImplementation),
            address(proxyAdmin),
            abi.encodeWithSignature("initialize(address,address)", wethAddress, address(this))
        );

        atomicBridgeInitiator = AtomicBridgeInitiator(address(proxy));
    }

    function testInitiateBridgeTransferWithEth() public {
        vm.deal(originator, 1 ether);
        vm.startPrank(originator);

        bytes32 bridgeTransferId = atomicBridgeInitiator.initiateBridgeTransfer{value: amount}(
            0, // _wethAmount
            recipient,
            hashLock,
            timeLock
        );

        (
            uint256 transferAmount,
            address transferOriginator,
            bytes32 transferRecipient,
            bytes32 transferHashLock,
            uint256 transferTimeLock,
            AtomicBridgeInitiator.MessageState transferState
        ) = atomicBridgeInitiator.bridgeTransfers(bridgeTransferId);

        assertEq(transferAmount, amount);
        assertEq(transferOriginator, originator);
        assertEq(transferRecipient, recipient);
        assertEq(transferHashLock, hashLock);
        assertGt(transferTimeLock, block.number);
        assertEq(uint8(transferState), uint8(AtomicBridgeInitiator.MessageState.INITIALIZED));

        vm.stopPrank();
    }

    function testCompleteBridgeTransfer() public {
        bytes32 secret = "secret";
        bytes32 testHashLock = keccak256(abi.encodePacked(secret));

        vm.deal(originator, 1 ether);
        vm.startPrank(originator);

        bytes32 bridgeTransferId = atomicBridgeInitiator.initiateBridgeTransfer{value: amount}(
            0, // _wethAmount is 0
            recipient,
            testHashLock,
            timeLock
        );

        vm.stopPrank();

        atomicBridgeInitiator.completeBridgeTransfer(bridgeTransferId, secret);
        (
            uint256 completedAmount,
            address completedOriginator,
            bytes32 completedRecipient,
            bytes32 completedHashLock,
            uint256 completedTimeLock,
            AtomicBridgeInitiator.MessageState completedState
        ) = atomicBridgeInitiator.bridgeTransfers(bridgeTransferId);

        assertEq(completedAmount, amount);
        assertEq(completedOriginator, originator);
        assertEq(completedRecipient, recipient);
        assertEq(completedHashLock, testHashLock);
        assertGt(completedTimeLock, block.number);
        assertEq(uint8(completedState), uint8(AtomicBridgeInitiator.MessageState.COMPLETED));
    }

    function testInitiateBridgeTransferWithWeth() public {
        uint256 wethAmount = 1 ether; // use ethers unit
        weth.totalSupply();
        vm.deal(originator, 1 ether);
        vm.startPrank(originator);
        weth.deposit{value: wethAmount}();
        assertEq(weth.balanceOf(originator), wethAmount);
        weth.approve(address(atomicBridgeInitiator), wethAmount);
        bytes32 bridgeTransferId =
            atomicBridgeInitiator.initiateBridgeTransfer(wethAmount, recipient, hashLock, timeLock);

        (
            uint256 transferAmount,
            address transferOriginator,
            bytes32 transferRecipient,
            bytes32 transferHashLock,
            uint256 transferTimeLock,
            AtomicBridgeInitiator.MessageState transferState
        ) = atomicBridgeInitiator.bridgeTransfers(bridgeTransferId);

        assertEq(transferAmount, wethAmount);
        assertEq(transferOriginator, originator);
        assertEq(transferRecipient, recipient);
        assertEq(transferHashLock, hashLock);
        assertGt(transferTimeLock, block.number);
        assertEq(uint8(transferState), uint8(AtomicBridgeInitiator.MessageState.INITIALIZED));

        vm.stopPrank();
    }

    function testInitiateBridgeTransferWithEthAndWeth() public {
        uint256 wethAmount = 1 ether;
        uint256 ethAmount = 2 ether;
        uint256 totalAmount = wethAmount + ethAmount;

        // Ensure the originator has sufficient ETH
        vm.deal(originator, 100 ether);

        vm.startPrank(originator);
        // Ensure WETH contract is correctly funded and transfer WETH to originator
        weth.deposit{value: wethAmount}();

        assertEq(weth.balanceOf(originator), wethAmount, "WETH balance mismatch");
        vm.expectRevert();
        // Try to initiate bridge transfer
        atomicBridgeInitiator.initiateBridgeTransfer{value: ethAmount}(wethAmount, recipient, hashLock, timeLock);
        // Try to initiate bridge transfer
        weth.approve(address(atomicBridgeInitiator), wethAmount);
        bytes32 bridgeTransferId =
            atomicBridgeInitiator.initiateBridgeTransfer{value: ethAmount}(wethAmount, recipient, hashLock, timeLock);

        // Fetch the details of the initiated bridge transfer
        (
            uint256 transferAmount,
            address transferOriginator,
            bytes32 transferRecipient,
            bytes32 transferHashLock,
            uint256 transferTimeLock,
            AtomicBridgeInitiator.MessageState transferState
        ) = atomicBridgeInitiator.bridgeTransfers(bridgeTransferId);

        // Assertions
        assertEq(transferAmount, totalAmount, "Transfer amount mismatch");
        assertEq(transferOriginator, originator, "Originator address mismatch");
        assertEq(transferRecipient, recipient, "Recipient address mismatch");
        assertEq(transferHashLock, hashLock, "HashLock mismatch");
        assertGt(transferTimeLock, block.number, "TimeLock is not greater than current block number");
        assertEq(uint8(transferState), uint8(AtomicBridgeInitiator.MessageState.INITIALIZED));

        vm.stopPrank();
    }

    function testRefundBridgeTransfer() public {
        vm.deal(originator, 1 ether);

        // Test unit owns atomicBridgeInitiator

        vm.startPrank(originator);
        bytes32 bridgeTransferId = atomicBridgeInitiator.initiateBridgeTransfer{value: amount}(
            0, // _wethAmount is 0
            recipient,
            hashLock,
            timeLock
        );
        vm.stopPrank();

        //Internally the timeLock is multiplied by 2 
        timeLock = timeLock * 4;

        // Advance time and block height to ensure the time lock has expired
        vm.warp(block.timestamp + timeLock + 1);
        uint256 futureTimeStamp = block.timestamp + timeLock + 4200;
        vm.roll(futureTimeStamp);

        vm.startPrank(originator);
        vm.expectRevert(abi.encodeWithSelector(OwnableUpgradeable.OwnableUnauthorizedAccount.selector, originator));
        // Call refund function
        atomicBridgeInitiator.refundBridgeTransfer(bridgeTransferId);
        vm.stopPrank();

        vm.expectEmit();
        emit IAtomicBridgeInitiator.BridgeTransferRefunded(bridgeTransferId);
        atomicBridgeInitiator.refundBridgeTransfer(bridgeTransferId);

        // Verify the WETH balance, originator should receive weth
        assertEq(weth.balanceOf(originator), 1 ether, "WETH balance mismatch");
    }
}

