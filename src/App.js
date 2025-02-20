import React from 'react';
import styled, { createGlobalStyle, keyframes } from 'styled-components';

// ===== Styled Components Updates =====
const scrollFade = keyframes`
  from { opacity: 0; transform: translateY(20px); }
  to { opacity: 1; transform: translateY(0); }
`;

const GlobalStyle = createGlobalStyle`
  /* Add bullet point fixes */
  ul {
    padding-left: 1.5rem;
    margin: 0.8rem 0;
    
    li {
      margin-bottom: 0.5rem;
      max-width: 80ch;
      line-height: 1.5;
    }
  }

  /* Mobile bullet alignment fix */
  @media (max-width: 768px) {
    ul {
      padding-left: 1.2rem;
      
      li {
        margin-left: 0.5rem;
      }
    }
  }
`;

const FeatureShowcase = styled.section`
  /* Add padding between image and features */
  margin: 4rem 0 2rem;
  animation: ${scrollFade} 0.8s ease;
`;

// ===== Updated Main Component =====
const App = () => {
  return (
    <>
      <GlobalStyle />
      <Container>
        <Header>
          <h1>Roblox Macro Utilities</h1>
          <p>Windows automation tool that works alongside Roblox - no memory access, just input automation</p>
          
          <ButtonGroup>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" className="primary">
              Download (1.3MB EXE)
            </Button>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities" className="secondary">
              Source Code
            </Button>
          </ButtonGroup>
        </Header>

        <FeatureShowcase>
          <div>
            <h2>Customizable Interface</h2>
            <p>Control how you interact with the tools:</p>
            <ul>
              <li>Drag buttons to any position</li>
              <li>Resize window to your preference</li>
              <li>Dark theme follows system settings</li>
              <li>Layout saves automatically</li>
            </ul>
          </div>
          <Screenshot>
            {/* Keep placeholder */}
          </Screenshot>
        </FeatureShowcase>

        <Section>
          <h2>Complete Feature List</h2>
          <FullFeatureList>
            <div>
              <h3>Essential Features</h3>
              <ul>
                <li>Persistent anti-AFK</li>
                <li>Single executable (no install)</li>
                <li>Update notifications</li>
                <li>Low resource usage</li>
              </ul>
            </div>

            <div>
              <h3>Movement Macros</h3>
              <ul>
                <li>Helicopter High Jump</li>
                <li>Speedglitch toggle</li>
                <li>Automatic Ledge Bouncing</li>
                <li>Wallhop/Wall-Walk</li>
                <li>14-Stud Lag High Jump</li>
              </ul>
            </div>

            <div>
              <h3>Advanced Functions</h3>
              <ul>
                <li>Item Desync Hitboxes</li>
                <li>Microsecond input timing</li>
                <li>Key spam repeater</li>
                <li>Process suspension</li>
                <li>Laugh Clipping</li>
              </ul>
            </div>

            <div>
              <h3>Technical Details</h3>
              <ul>
                <li>External input simulation</li>
                <li>Portable execution</li>
                <li>Open source C++</li>
                <li>Active maintenance</li>
              </ul>
            </div>
          </FullFeatureList>
        </Section>

        <FeatureShowcase>
          <Screenshot>
            {/* Keep placeholder */}
          </Screenshot>
          <div>
            <h2>Precise Control</h2>
            <p>Detailed configuration options:</p>
            <ul>
              <li>Microsecond timing accuracy</li>
              <li>Custom keybind mapping</li>
              <li>Active macro indicators</li>
              <li>Toggle state visibility</li>
            </ul>
          </div>
        </FeatureShowcase>

        <Section style={{ textAlign: 'center', marginTop: '4rem' }}>
          <h2>Get Started</h2>
          <ButtonGroup>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" className="primary">
              Download (1.3MB)
            </Button>
            <Button href="https://discord.gg/roblox-glitching-community-998572881892094012" className="secondary">
              Join Community
            </Button>
          </ButtonGroup>
          <p style={{ marginTop: '1rem', opacity: 0.8 }}>
            Windows 10/11 · Portable EXE · No dependencies
          </p>
          <p style={{ marginTop: '1rem', opacity: 0.7, fontSize: '0.9rem' }}>
            Discord features update pings and glitch discussion
          </p>
        </Section>

        <Credits>
          <p>Implementation Details:</p>
          <ul>
            <li><a href="https://github.com/ocornut/imgui">ImGui</a> interface framework</li>
            <li><a href="https://github.com/craftwar/suspend">suspend</a> process handling</li>
          </ul>
        </Credits>
      </Container>
    </>
  );
};

export default App;
