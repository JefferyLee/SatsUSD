package main

import (
	"bytes"
	"encoding/hex"
	"fmt"
	"os"

	"github.com/btcsuite/btcd/txscript"
	"github.com/btcsuite/btcd/wire"
	"github.com/lightninglabs/taproot-assets/asset"
	"github.com/lightninglabs/taproot-assets/commitment"
)

func main() {
	raw, _ := hex.DecodeString(readHex(os.Args[1]))
	var a asset.Asset
	if err := a.Decode(bytes.NewReader(raw)); err != nil {
		panic(err)
	}
	if a.HasSplitCommitmentWitness() {
		a = *a.Copy()
		a.PrevWitnesses[0].SplitCommitment = nil
	}
	ac, _ := commitment.NewAssetCommitment(&a)
	v2 := commitment.TapCommitmentV2
	tc, _ := commitment.NewTapCommitment(&v2, ac)
	noSib := tc.TapscriptRoot(nil)
	fmt.Println("ta_leaf_hash:", hex.EncodeToString(noSib[:]))

	// --- leaf sibling: a simple non-TA-commitment tapscript (OP_TRUE) ---
	leaf := txscript.NewBaseTapLeaf([]byte{0x51})
	leafPre, err := commitment.NewPreimageFromLeaf(leaf)
	if err != nil {
		panic(err)
	}
	emitSibling("LEAF", leafPre, tc)

	// --- branch sibling: branch of two leaves ---
	l2 := txscript.NewBaseTapLeaf([]byte{0x52})
	branch := txscript.NewTapBranch(leaf, l2)
	branchPre := commitment.NewPreimageFromBranch(branch)
	emitSibling("BRANCH", &branchPre, tc)
}

func emitSibling(label string, pre *commitment.TapscriptPreimage,
	tc *commitment.TapCommitment) {

	encoded, tapHash, err := commitment.MaybeEncodeTapscriptPreimage(pre)
	if err != nil {
		panic(err)
	}
	root := tc.TapscriptRoot(tapHash)
	fmt.Printf("%s_type5_wire: %s\n", label, hex.EncodeToString(encoded))
	fmt.Printf("%s_sibling_taphash: %s\n", label, hex.EncodeToString(tapHash[:]))
	fmt.Printf("%s_tapscript_root: %s\n", label, hex.EncodeToString(root[:]))
}

func readHex(p string) string { b, _ := os.ReadFile(p); return string(bytes.TrimSpace(b)) }

var _ = wire.ReadVarBytes
