package microblossom.modules

import spinal.core._
import spinal.lib._
import microblossom._
import microblossom.types._
import microblossom.stage._
import org.scalatest.funsuite.AnyFunSuite

case class Vertex(config: DualConfig, vertexIndex: Int, injectRegisters: Seq[String] = List()) extends Component {
  val io = new Bundle {
    val message = in(BroadcastMessage(config))
    val debugState = out(VertexState(config.vertexBits, config.grownBitsOf(vertexIndex)))
  }

  val stages = Stages(
    offload = () => StageOffloadVertex(config, vertexIndex),
    offload2 = () => StageOffloadVertex2(config, vertexIndex),
    offload3 = () => StageOffloadVertex3(config, vertexIndex),
    offload4 = () => StageOffloadVertex4(config, vertexIndex)
  )

  // fetch
  var ram: Mem[VertexState] = null
  var register = Reg(VertexState(config.vertexBits, config.grownBitsOf(vertexIndex)))
  var fetchState = VertexState(config.vertexBits, config.grownBitsOf(vertexIndex))
  var message = BroadcastMessage(config)
  if (config.contextBits > 0) {
    // fetch stage, delay the instruction
    ram = Mem(VertexState(config.vertexBits, config.grownBitsOf(vertexIndex)), config.contextDepth)
    fetchState := ram.readSync(
      address = io.message.contextId,
      enable = io.message.valid
    )
    message := RegNext(io.message)
  } else {
    fetchState := register
    message := io.message
  }

  // mock
  stages.offloadSet.state := fetchState
  stages.offloadSet2.state := stages.offloadGet.state
  stages.offloadSet3.state := stages.offloadGet2.state
  stages.offloadSet4.state := stages.offloadGet3.state
  register := stages.offloadGet4.state

  // inject registers
  for (stageName <- injectRegisters) {
    stages.injectRegisterAt(stageName)
  }
  stages.finish()

  io.debugState := stages.offloadGet4.state

}

// sbt 'testOnly microblossom.modules.VertexTest'
class VertexTest extends AnyFunSuite {

  test("construct a Vertex") {
    val config = DualConfig(filename = "./resources/graphs/example_code_capacity_d3.json")
    // config.contextDepth = 1024 // fit in a single Block RAM of 36 kbits in 36-bit mode
    config.contextDepth = 1 // no context switch
    config.sanityCheck()
    Config.spinal().generateVerilog(Vertex(config, 0))
  }

}
